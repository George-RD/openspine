use crate::api::actions::DispatchError;
use crate::api::connector_breaker::{call_with_connector, call_with_connector_write};
use crate::artifact_store::ArtifactStoreError;
use openspine_schemas::action::ActionRequest;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::digest_of;
use openspine_schemas::grant::TaskGrant;
use serde_json::json;
use ulid::Ulid;

use super::{notify_owner_best_effort, AppState};

/// Actually create the Gmail draft after `gate()` confirms a matching,
/// unexpired approval. Re-derives the recipient from a live Gmail fetch and
/// re-checks it against the proposal-bound digest before calling
/// `create_draft`, because a new thread message can change the recipient.
/// Candidate Gmail-write extension: a write timeout/no-response leaves
/// durable pending evidence for manual/operator reconciliation; no automatic
/// resend is safe without Gmail idempotency.
pub(crate) async fn create_approved_draft(
    state: &AppState,
    grant: &TaskGrant,
    request: &ActionRequest,
    chat_id: i64,
) -> anyhow::Result<()> {
    let payload_ref = request
        .payload_ref
        .as_ref()
        .expect("checked by handle_draft_approval_callback before dispatch");
    let bytes = match state.artifacts.get(payload_ref) {
        Ok(bytes) => bytes,
        Err(ArtifactStoreError::DigestMismatch) => {
            state.store.append_audit(
                "draft.payload_mutated_since_approval",
                Some(&request.action),
                None,
                Some("recomputed payload digest no longer matches the approved one"),
                Some(grant.id),
                &[],
                std::slice::from_ref(payload_ref),
            )?;
            notify_owner_best_effort(
                state,
                chat_id,
                "The draft content changed since you approved it — please run /draft again.",
            )
            .await;
            return Ok(());
        }
        Err(other) => return Err(other.into()),
    };
    let payload: serde_json::Value = serde_json::from_slice(&bytes)?;
    let subject = payload["subject"].as_str().unwrap_or_default();
    let body = payload["body"].as_str().unwrap_or_default();
    let thread_id = request
        .target_ref
        .as_ref()
        .and_then(|t| t.id.clone())
        .unwrap_or_default();

    let Some(gmail) = state.connectors.gmail() else {
        state.store.append_audit(
            "draft.creation_failed",
            Some(&request.action),
            None,
            Some("no gmail connector configured"),
            Some(grant.id),
            &[],
            &[],
        )?;
        crate::failure_surfacing::batch_failure(
            state,
            crate::failure_surfacing::FailureClass::Connector,
            "gmail connector unavailable during approval",
            "gmail connector unavailable during approval",
        )?;
        return Ok(());
    };

    crate::spend::guard_connector_for(state, grant).await?;
    let thread = match call_with_connector(
        state,
        "gmail",
        &request.action,
        grant,
        gmail.fetch_thread(&thread_id),
    )
    .await
    {
        Ok(thread) => thread,
        Err(DispatchError::ConnectorUnavailable(err)) => return Err(err),
        Err(err) => {
            state.store.append_audit(
                "draft.creation_failed",
                Some(&request.action),
                None,
                None,
                Some(grant.id),
                &[],
                &[],
            )?;
            crate::failure_surfacing::batch_failure(
                state,
                crate::failure_surfacing::FailureClass::Connector,
                "gmail thread fetch failed during approval",
                &format!("{err:?}"),
            )?;
            return Ok(());
        }
    };
    let Some(target) = crate::gmail::newest_non_owner_recipient(&thread, gmail.mailbox_address())
    else {
        state.store.append_audit(
            "draft.creation_failed",
            Some(&request.action),
            None,
            Some("no non-owner recipient found in thread"),
            Some(grant.id),
            &[],
            &[],
        )?;
        notify_owner_best_effort(
            state,
            chat_id,
            "Approved, but couldn't determine who to reply to.",
        )
        .await;
        return Ok(());
    };

    let current_target_digest = digest_of(&json!({
        "thread_id": thread_id,
        "connector": "gmail_primary",
        "account_role": "owner_mailbox",
        "recipients": [target.recipient],
    }));
    if Some(&current_target_digest) != request.target_digest.as_ref() {
        let target_ref = ArtifactRef {
            digest: current_target_digest.clone(),
            schema_version: 1,
        };
        state.store.append_audit(
            "draft.target_mutated_since_approval",
            Some(&request.action),
            None,
            Some("recomputed target digest no longer matches the approved one"),
            Some(grant.id),
            &[target_ref],
            std::slice::from_ref(payload_ref),
        )?;
        notify_owner_best_effort(
            state,
            chat_id,
            "The thread changed since you approved this draft — please run /draft again.",
        )
        .await;
        return Ok(());
    }

    crate::spend::guard_connector_for(state, grant).await?;
    // Candidate Gmail-write extension: persist durable pending evidence before
    // touching Gmail. Timeout/no-response remains pending; no automatic resend
    // is safe because Gmail create_draft has no idempotency key.
    let pending_id = Ulid::new();
    state
        .store
        .insert_pending_draft_write(pending_id, grant.id, request.id, &thread_id)?;
    let draft_result = call_with_connector_write(
        state,
        "gmail",
        &request.action,
        grant,
        gmail.create_draft(&thread_id, &target, subject, body),
    )
    .await;
    if let Err(DispatchError::DeliveryUnknown(err)) = &draft_result {
        state.store.append_audit(
            "draft.delivery_unknown",
            Some(&request.action),
            None,
            Some(&err.to_string()),
            Some(grant.id),
            &[],
            std::slice::from_ref(payload_ref),
        )?;
        return Ok(());
    }
    match draft_result {
        Ok(draft_id) => {
            state.store.resolve_pending_draft_write(pending_id)?;
            let draft_id_refs = match state.artifacts.put(draft_id.as_bytes()) {
                Ok(r) => vec![r],
                Err(err) => {
                    tracing::warn!(error = %err, "failed to store draft_id artifact ref");
                    vec![]
                }
            };
            let target_ref = ArtifactRef {
                digest: current_target_digest.clone(),
                schema_version: 1,
            };
            let mut payload_refs = vec![payload_ref.clone()];
            payload_refs.extend(draft_id_refs);
            state.store.append_audit(
                "draft.created",
                Some(&request.action),
                None,
                None,
                Some(grant.id),
                &[target_ref],
                &payload_refs,
            )?;
            notify_owner_best_effort(state, chat_id, "Draft created in Gmail.").await;
        }
        Err(DispatchError::ConnectorUnavailable(err)) => {
            state.store.resolve_pending_draft_write(pending_id)?;
            return Err(err);
        }
        Err(DispatchError::DeliveryUnknown(_)) => unreachable!("handled above"),
        Err(err) => {
            state.store.resolve_pending_draft_write(pending_id)?;
            let target_ref = ArtifactRef {
                digest: current_target_digest.clone(),
                schema_version: 1,
            };
            state.store.append_audit(
                "draft.creation_failed",
                Some(&request.action),
                None,
                None,
                Some(grant.id),
                &[target_ref],
                std::slice::from_ref(payload_ref),
            )?;
            crate::failure_surfacing::batch_failure(
                state,
                crate::failure_surfacing::FailureClass::Connector,
                "gmail create draft failed during approval",
                &format!("{err:?}"),
            )?;
        }
    }
    Ok(())
}
