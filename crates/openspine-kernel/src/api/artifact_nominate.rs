//! `artifact.nominate_upstream` dispatch (AD-071): propose a learned overlay
//! artifact as an upstream candidate. The request is a normal digest-bound
//! approval; the `depersonalized` assertion is captured in the bound payload
//! and re-verified kernel-side at finalization, never trusted from the caller
//! alone.

use jiff::Timestamp;
use openspine_schemas::action::{ActionId, ActionRequest};
use openspine_schemas::digest::digest_of;
use openspine_schemas::digest::digest_of_bytes;
use openspine_schemas::grant::TaskGrant;
use serde::Deserialize;
use serde_json::{json, Value};
use ulid::Ulid;

use super::actions::DispatchError;
use super::connector_breaker::call_with_connector;
use crate::pipeline::AppState;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactNominatePayload {
    kind: String,
    artifact_id: String,
    version: u32,
    depersonalized: bool,
}

pub(super) async fn dispatch_artifact_nominate(
    state: &AppState,
    grant: &TaskGrant,
    bound_chat_id: i64,
    payload: Option<&Value>,
) -> Result<Value, DispatchError> {
    let payload = payload.ok_or_else(|| {
        DispatchError::BadRequest("artifact.nominate_upstream requires a payload".to_string())
    })?;
    let req: ArtifactNominatePayload =
        serde_json::from_value(payload.clone()).map_err(|_| {
            DispatchError::BadRequest(
                "artifact.nominate_upstream payload must be {kind, artifact_id, version, depersonalized}"
                    .to_string(),
            )
        })?;
    if !req.depersonalized {
        return Err(DispatchError::BadRequest(
            "artifact.nominate_upstream requires depersonalized: true".to_string(),
        ));
    }
    if !crate::artifact_loader::is_proposable_kind(&req.kind) {
        return Err(DispatchError::BadRequest(
            "artifact.nominate_upstream target kind is not proposable".to_string(),
        ));
    }
    let learned = state
        .store
        .list_learned_artifacts()
        .map_err(|err| DispatchError::Resource(err.into()))?;
    let exists_compatible = learned.iter().any(|item| {
        item.kind == req.kind
            && item.artifact_id == req.artifact_id
            && item.version == req.version
            && item.compatibility
                == crate::store::learned_artifacts::CompatibilityStatus::Compatible
    });
    if !exists_compatible {
        return Err(DispatchError::BadRequest(
            "artifact.nominate_upstream target is not a compatible learned artifact".to_string(),
        ));
    }
    let subdir = crate::artifact_loader::find_kind_spec(&req.kind)
        .map(|spec| spec.overlay_subdir)
        .unwrap_or("");
    let content_path =
        state
            .overlay_dir
            .join(subdir)
            .join(crate::artifact_loader::overlay_filename(
                &req.artifact_id,
                req.version,
            ));
    let content_bytes = std::fs::read(&content_path).map_err(|err| {
        DispatchError::BadRequest(format!(
            "artifact.nominate_upstream target content is unavailable: {err}"
        ))
    })?;
    let content_digest = digest_of_bytes(&content_bytes).as_str().to_string();

    let assertion = json!({
        "kind": req.kind,
        "artifact_id": req.artifact_id,
        "version": req.version,
        "depersonalized": true,
        "content_digest": content_digest,
    });
    let assertion_bytes =
        serde_json::to_vec(&assertion).map_err(|err| DispatchError::Resource(err.into()))?;
    let payload_ref = state
        .artifacts
        .put(&assertion_bytes)
        .map_err(|err| DispatchError::Resource(err.into()))?;
    let target_digest = digest_of(&assertion);
    let now = Timestamp::now();
    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant.id,
        action: ActionId::new("artifact.nominate_upstream"),
        target_ref: None,
        payload_ref: Some(payload_ref.clone()),
        target_digest: Some(target_digest.clone()),
        selection_token_id: None,
        params: std::collections::BTreeMap::new(),
        requested_at: now,
        schema_version: 1,
    };
    state
        .store
        .insert_action_request(&request)
        .map_err(|err| DispatchError::Resource(err.into()))?;
    let summary = format!(
        "Nominate upstream\nKind: {}\nId: {} v{}\nDepersonalized: true\nContent digest: {}\nApproval digest: {}\n\nApprove to nominate.",
        req.kind,
        req.artifact_id,
        req.version,
        content_digest,
        target_digest,
    );
    crate::spend::guard_connector_for(state, grant)
        .await
        .map_err(DispatchError::Resource)?;
    call_with_connector(
        state,
        "telegram",
        &request.action,
        grant,
        state.connectors.telegram().send_reply_with_approval_button(
            bound_chat_id,
            &summary,
            request.id,
        ),
    )
    .await?;
    Ok(json!({
        "proposed": true,
        "action_request_id": request.id.to_string(),
    }))
}
