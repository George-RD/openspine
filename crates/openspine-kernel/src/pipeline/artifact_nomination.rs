//! Post-approval handler for `artifact.nominate_upstream` (AD-071). The
//! depersonalized opt-in is re-verified from the bound, content-addressed
//! assertion bytes — never a caller-supplied value.

use openspine_schemas::action::ActionRequest;
use openspine_schemas::grant::TaskGrant;
use serde::Deserialize;

use super::{notify_owner_best_effort, AppState};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NominationAssertion {
    kind: String,
    artifact_id: String,
    version: u32,
    depersonalized: bool,
    content_digest: String,
}

pub(super) async fn finalize_nomination(
    state: &AppState,
    grant: &TaskGrant,
    request: &ActionRequest,
    chat_id: i64,
) -> anyhow::Result<()> {
    let Some(payload_ref) = request.payload_ref.as_ref() else {
        state.store.append_audit(
            "artifact.nomination_malformed",
            Some(&request.action),
            None,
            Some("nomination request has no payload ref"),
            Some(grant.id),
            &[],
            &[],
        )?;
        return Ok(());
    };
    let bytes = state.artifacts.get(payload_ref)?;
    let assertion: NominationAssertion = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(err) => {
            state.store.append_audit(
                "artifact.nomination_malformed",
                Some(&request.action),
                None,
                Some("nomination payload failed to parse"),
                Some(grant.id),
                &[],
                &[],
            )?;
            tracing::warn!(error = %err, "nomination payload failed to parse");
            return Ok(());
        }
    };
    if !assertion.depersonalized {
        state.store.append_audit(
            "artifact.nomination_not_depersonalized",
            Some(&request.action),
            None,
            Some("nomination assertion is not depersonalized"),
            Some(grant.id),
            &[],
            &[],
        )?;
        notify_owner_best_effort(
            state,
            chat_id,
            "Nomination refused — content must be marked depersonalized.",
        )
        .await;
        return Ok(());
    }
    let subdir = crate::artifact_loader::find_kind_spec(&assertion.kind)
        .map(|spec| spec.overlay_subdir)
        .unwrap_or("");
    let content_path =
        state
            .overlay_dir
            .join(subdir)
            .join(crate::artifact_loader::overlay_filename(
                &assertion.artifact_id,
                assertion.version,
            ));
    let current_digest = match std::fs::read(&content_path) {
        Ok(bytes) => openspine_schemas::digest::digest_of_bytes(&bytes)
            .as_str()
            .to_string(),
        Err(_) => String::new(),
    };
    if current_digest != assertion.content_digest {
        state.store.append_audit(
            "artifact.nomination_content_mismatch",
            Some(&request.action),
            None,
            Some("artifact content changed after nomination review"),
            Some(grant.id),
            &[],
            std::slice::from_ref(payload_ref),
        )?;
        notify_owner_best_effort(
            state,
            chat_id,
            "Nomination refused — artifact content changed after review.",
        )
        .await;
        return Ok(());
    }
    match state.store.nominate_upstream(
        &assertion.kind,
        &assertion.artifact_id,
        assertion.version,
        true,
    ) {
        Ok(()) => {
            state.store.append_audit(
                "artifact.nominated",
                Some(&request.action),
                None,
                None,
                Some(grant.id),
                &[],
                std::slice::from_ref(payload_ref),
            )?;
            notify_owner_best_effort(
                state,
                chat_id,
                &format!(
                    "Artifact {} v{} nominated for upstream review.",
                    assertion.artifact_id, assertion.version
                ),
            )
            .await;
        }
        Err(err) => {
            state.store.append_audit(
                "artifact.nomination_failed",
                Some(&request.action),
                None,
                Some(&err.to_string()),
                Some(grant.id),
                &[],
                &[],
            )?;
            notify_owner_best_effort(state, chat_id, "Nomination could not be completed.").await;
        }
    }
    Ok(())
}
