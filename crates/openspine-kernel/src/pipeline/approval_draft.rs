use crate::api::actions::DispatchError;
use crate::api::connector_breaker::call_with_connector;
use crate::api::effect_executors::EffectDisposition;
use crate::artifact_store::ArtifactStoreError;
use openspine_schemas::action::ActionRequest;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::digest_of;
use openspine_schemas::grant::TaskGrant;
use serde_json::json;
use ulid::Ulid;

use super::{notify_owner_best_effort, AppState};
use crate::store::{AuditDescriptor, BeginEffect, PendingWriteFence};
use openspine_schemas::owner_surface::OwnerSurfaceRef;

/// Actually create the Gmail draft after `gate()` confirms a matching,
/// unexpired approval. Re-derives the recipient from a live Gmail fetch and
/// re-checks it against the proposal-bound digest before calling
/// `create_draft`, because a new thread message can change the recipient.
/// Provider-boundary contract: connector admission is taken before the pending
/// fence and before polling any provider future. A rejected admission is
/// [`EffectDisposition::NotAttempted`]. Once the write future is polled, a
/// confirmed Gmail success is [`EffectDisposition::ConfirmedSuccess`], a
/// definite client rejection is [`EffectDisposition::ConfirmedFailure`], and
/// an outcome that does not prove non-occurrence (timeout, transport/malformed
/// response, HTTP 429, or HTTP 5xx) is [`EffectDisposition::DeliveryUnknown`]
/// with durable pending
/// evidence left open for reconciliation. No automatic resend is safe without
/// Gmail idempotency.
pub(crate) async fn create_approved_draft(
    state: &AppState,
    grant: &TaskGrant,
    request: &ActionRequest,
    owner_surface: &OwnerSurfaceRef,
) -> anyhow::Result<EffectDisposition> {
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
                owner_surface,
                "The draft content changed since you approved it — please run /draft again.",
            )
            .await;
            return Ok(EffectDisposition::NotAttempted);
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
        return Ok(EffectDisposition::NotAttempted);
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
        Err(DispatchError::ConnectorUnavailable(_)) => {
            return Ok(EffectDisposition::NotAttempted);
        }
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
            return Ok(EffectDisposition::NotAttempted);
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
            owner_surface,
            "Approved, but couldn't determine who to reply to.",
        )
        .await;
        return Ok(EffectDisposition::NotAttempted);
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
            owner_surface,
            "The thread changed since you approved this draft — please run /draft again.",
        )
        .await;
        return Ok(EffectDisposition::NotAttempted);
    }
    let request_fingerprint = crate::store::draft_request_fingerprint(
        request.action.as_str(),
        &thread_id,
        &current_target_digest,
        &payload_ref.digest,
    );
    if state.store.has_pending_draft_write(&request_fingerprint)? {
        state.store.append_audit(
            "draft.pending_write_fenced",
            Some(&request.action),
            None,
            Some("an unresolved provider write already exists for this request"),
            Some(grant.id),
            &[],
            std::slice::from_ref(payload_ref),
        )?;
        notify_owner_best_effort(
            state,
            owner_surface,
            "This draft write is still awaiting Gmail reconciliation; no retry was sent.",
        )
        .await;
        return Ok(EffectDisposition::NotAttempted);
    }

    crate::spend::guard_connector_for(state, grant).await?;

    // Take the breaker/rate-limit permit BEFORE writing the pending-write
    // fence. A rejected admission has polled no *write* future, so no draft
    // was ever sent: it must not leave (or resolve) durable pending evidence,
    // and it must not be reported as a failed write attempt. The read-only
    // thread fetch above already ran; that is a read, not an effect.
    let permit = match crate::api::connector_breaker::admit_connector_write(
        state,
        "gmail",
        &request.action,
        grant,
    ) {
        Ok(permit) => permit,
        Err(DispatchError::ConnectorUnavailable(_)) => {
            return Ok(EffectDisposition::NotAttempted);
        }
        Err(err) => {
            state.store.append_audit(
                "draft.creation_failed",
                Some(&request.action),
                None,
                Some("gmail write admission rejected before any provider write"),
                Some(grant.id),
                &[],
                std::slice::from_ref(payload_ref),
            )?;
            crate::failure_surfacing::batch_failure(
                state,
                crate::failure_surfacing::FailureClass::Connector,
                "gmail create draft admission rejected during approval",
                &format!("{err:?}"),
            )?;
            return Ok(EffectDisposition::NotAttempted);
        }
    };
    // Candidate Gmail-write extension: persist durable pending evidence before
    // touching Gmail. Timeout/no-response remains pending; no automatic resend
    // is safe because Gmail create_draft has no idempotency key.
    //
    // The claim repeats the earlier fast-path read atomically with insertion.
    // Two concurrent callbacks can both observe the fast path as open, but
    // only one can win this `BEGIN IMMEDIATE` claim before touching Gmail.
    let pending_id = Ulid::new();
    let mut begin_audit = AuditDescriptor::new("draft.pending_write_opened")
        .with_reason("pending-write fence claimed before the gmail draft write");
    begin_audit.action = Some(request.action.clone());
    begin_audit.task_grant_id = Some(grant.id);
    begin_audit.payload_refs = vec![payload_ref.clone()];
    let fence = match state.store.begin_effect(
        PendingWriteFence {
            id: pending_id,
            grant_id: grant.id,
            action_request_id: request.id,
            thread_id: &thread_id,
            request_fingerprint: &request_fingerprint,
        },
        begin_audit,
    )? {
        BeginEffect::Fenced(fence) => fence,
        BeginEffect::AlreadyFenced => {
            // Dropping an unused permit is intentional: a half-open probe must
            // not remain armed when no provider future was polled.
            drop(permit);
            state.store.append_audit(
                "draft.pending_write_fenced",
                Some(&request.action),
                None,
                Some("an unresolved provider write already exists for this request"),
                Some(grant.id),
                &[],
                std::slice::from_ref(payload_ref),
            )?;
            notify_owner_best_effort(
                state,
                owner_surface,
                "This draft write is still awaiting Gmail reconciliation; no retry was sent.",
            )
            .await;
            return Ok(EffectDisposition::NotAttempted);
        }
    };
    let draft_result = crate::api::connector_breaker::call_with_admitted_connector_write(
        state,
        "gmail",
        &request.action,
        permit,
        gmail.create_draft(&thread_id, &target, subject, body),
    )
    .await;
    // Map the connector outcome to an `EffectDisposition` and settle through the
    // seam. This inline mapping is a minimal choice for the pilot SHAPE only;
    // Effect Truth #198 owns the truthful connector-outcome classification.
    if let Err(DispatchError::DeliveryUnknown(err)) = &draft_result {
        let mut audit = AuditDescriptor::new("draft.delivery_unknown").with_reason(err.to_string());
        audit.action = Some(request.action.clone());
        audit.task_grant_id = Some(grant.id);
        audit.payload_refs = vec![payload_ref.clone()];
        state
            .store
            .settle_effect(fence, EffectDisposition::DeliveryUnknown, audit)?;
        return Ok(EffectDisposition::DeliveryUnknown);
    }
    match draft_result {
        Ok(draft_id) => {
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
            let mut audit = AuditDescriptor::new("draft.created");
            audit.action = Some(request.action.clone());
            audit.task_grant_id = Some(grant.id);
            audit.target_refs = vec![target_ref];
            audit.payload_refs = payload_refs;
            state
                .store
                .settle_effect(fence, EffectDisposition::ConfirmedSuccess, audit)?;
            notify_owner_best_effort(state, owner_surface, "Draft created in Gmail.").await;
            Ok(EffectDisposition::ConfirmedSuccess)
        }
        Err(DispatchError::DeliveryUnknown(_)) => unreachable!("handled above"),
        Err(err) => {
            let target_ref = ArtifactRef {
                digest: current_target_digest.clone(),
                schema_version: 1,
            };
            let mut audit = AuditDescriptor::new("draft.creation_failed");
            audit.action = Some(request.action.clone());
            audit.task_grant_id = Some(grant.id);
            audit.target_refs = vec![target_ref];
            audit.payload_refs = vec![payload_ref.clone()];
            state
                .store
                .settle_effect(fence, EffectDisposition::ConfirmedFailure, audit)?;
            crate::failure_surfacing::batch_failure(
                state,
                crate::failure_surfacing::FailureClass::Connector,
                "gmail create draft failed during approval",
                &format!("{err:?}"),
            )?;
            Ok(EffectDisposition::ConfirmedFailure)
        }
    }
}

/// The single kernel-owned Gmail draft executor addressed by the D-146
/// `executor_id` `"gmail.create_draft"`, shared by every admission source.
pub(crate) fn gmail_create_draft_executor<'a>(
    state: &'a AppState,
    grant: &'a TaskGrant,
    request: &'a ActionRequest,
    owner_surface: &'a OwnerSurfaceRef,
) -> crate::api::effect_executors::EffectExecutorFuture<'a> {
    Box::pin(create_approved_draft(state, grant, request, owner_surface))
}
