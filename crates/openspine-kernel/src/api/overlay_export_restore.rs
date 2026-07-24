//! Root-owner overlay export/restore action handlers.
//!
//! Both actions stage a restart-bound operation through the AppState-facing
//! overlay operations seam and never copy or replace open storage.

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::grant::TaskGrant;
use serde::Deserialize;
use serde_json::{json, Value};

use super::actions::DispatchError;
use super::handler_registry::HandlerFuture;
use crate::overlay_export_restore::ControlError;
use crate::pipeline::AppState;

const EXPORT_ACTION: &str = "openspine.overlay.export";
const RESTORE_ACTION: &str = "openspine.overlay.restore";

/// Strict one-field payload for export/restore requests.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleNamePayload {
    bundle_name: String,
}

fn parse_payload(action: &str, payload: Option<&Value>) -> Result<String, DispatchError> {
    let payload =
        payload.ok_or_else(|| DispatchError::BadRequest(format!("{action} requires a payload")))?;
    let req: BundleNamePayload = serde_json::from_value(payload.clone()).map_err(|_| {
        DispatchError::BadRequest(format!(
            "{action} payload must be exactly {shape}",
            shape = r#"{"bundle_name": string}"#
        ))
    })?;
    Ok(req.bundle_name)
}

fn require_root_owner_grant(
    state: &AppState,
    grant: &TaskGrant,
    action: &ActionId,
) -> Result<(), DispatchError> {
    if grant.user != state.owner_principal_id.to_string() {
        return Err(DispatchError::BadRequest(
            "overlay export/restore requires the configured owner principal".to_string(),
        ));
    }

    let is_root = grant.parent_grant_id.is_none()
        && grant.root_grant_id == grant.id
        && matches!(
            grant.chain.as_slice(),
            [root]
                if root.grant_id == grant.id
                    && root.parent_grant_id.is_none()
        );
    if !is_root {
        return Err(DispatchError::BadRequest(
            "overlay export/restore requires a root grant with no delegated hops".to_string(),
        ));
    }

    if !state.action_catalog.is_non_delegable(action) {
        return Err(DispatchError::BadRequest(
            "overlay export/restore requires non-delegable action classification".to_string(),
        ));
    }

    if !grant.effectively_allows(action) {
        return Err(DispatchError::BadRequest(
            "overlay export/restore requires exact effective authority for the action".to_string(),
        ));
    }

    Ok(())
}

fn map_control_error(err: ControlError) -> DispatchError {
    if matches!(err, ControlError::Io { .. }) {
        DispatchError::Resource(anyhow::Error::new(err))
    } else {
        DispatchError::BadRequest(err.to_string())
    }
}

fn stage(
    state: &AppState,
    grant: &TaskGrant,
    action: &ActionId,
    bundle_name: &str,
) -> Result<Value, DispatchError> {
    require_root_owner_grant(state, grant, action)?;
    state
        .overlay_operations
        .stage_export_or_restore(grant, action, bundle_name, Timestamp::now())
        .map_err(map_control_error)?;
    Ok(json!({
        "restart_required": true,
        "bundle_name": bundle_name,
        "action": action.as_str(),
    }))
}

pub(crate) fn handle_overlay_export<'a>(
    state: &'a AppState,
    grant: &'a TaskGrant,
    action: &'a ActionId,
    _chat_id: i64,
    payload: Option<&'a Value>,
) -> HandlerFuture<'a> {
    Box::pin(async move {
        if action.as_str() != EXPORT_ACTION {
            return Err(DispatchError::BadRequest(
                "handler registered for openspine.overlay.export only".to_string(),
            ));
        }
        let bundle_name = parse_payload(EXPORT_ACTION, payload)?;
        stage(state, grant, action, &bundle_name)
    })
}

pub(crate) fn handle_overlay_restore<'a>(
    state: &'a AppState,
    grant: &'a TaskGrant,
    action: &'a ActionId,
    _chat_id: i64,
    payload: Option<&'a Value>,
) -> HandlerFuture<'a> {
    Box::pin(async move {
        if action.as_str() != RESTORE_ACTION {
            return Err(DispatchError::BadRequest(
                "handler registered for openspine.overlay.restore only".to_string(),
            ));
        }
        let bundle_name = parse_payload(RESTORE_ACTION, payload)?;
        stage(state, grant, action, &bundle_name)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::dispatch_tests::OWNER_CHAT_ID;
    use crate::pipeline::handle_owner_update;
    use crate::test_support::fixtures::{owner_update, test_state};
    use openspine_schemas::artifact::Lifecycle;
    use openspine_schemas::grant::{GrantLimits, GrantMode};
    use ulid::Ulid;

    fn mint_grant(
        state: &AppState,
        user: String,
        action: &str,
        parent: Option<Ulid>,
        chain_nonempty: bool,
    ) -> TaskGrant {
        let now = Timestamp::now();
        let mut grant = TaskGrant {
            id: Ulid::new(),
            schema_version: 1,
            lifecycle_state: Lifecycle::Active,
            user,
            purpose: "owner_control".to_string(),
            issued_by: "kernel".to_string(),
            issued_at: now,
            expires_at: now + std::time::Duration::from_secs(120),
            event_id: Ulid::new(),
            route_id: "owner_telegram_main_assistant".to_string(),
            agent_id: "main_assistant_agent".to_string(),
            workflow_id: "owner_control_conversation".to_string(),
            capability_pack_id: "owner_control_basic_pack".to_string(),
            authority_sources: vec![],
            selection_tokens: vec![],
            allowed_actions: vec![ActionId::new(action)],
            approval_required_actions: vec![],
            denied_actions: vec![],
            allowed_egress_classes: vec![],
            output_channels: vec![],
            limits: GrantLimits {
                max_model_calls: 8,
                max_artifacts: 20,
                max_runtime_seconds: 120,
            },
            task_token: Ulid::new().to_string(),
            root_grant_id: Ulid::nil(),
            parent_grant_id: parent,
            mode: GrantMode::Live,
            chain: vec![],
            caveat_mac: String::new(),
            thread_id: None,
            persona_id: None,
        };
        grant.seal_root(b"openspine-test-grant-hmac-key-v1");
        if let Some(root_id) = parent {
            grant.root_grant_id = root_id;
            grant.parent_grant_id = Some(root_id);
            grant.chain = vec![
                openspine_schemas::grant_chain::ChainStep {
                    grant_id: root_id,
                    parent_grant_id: None,
                    mode: GrantMode::Live,
                    selection_tokens: vec![],
                    added_caveats: vec![],
                },
                openspine_schemas::grant_chain::ChainStep {
                    grant_id: grant.id,
                    parent_grant_id: Some(root_id),
                    mode: GrantMode::Live,
                    selection_tokens: vec![],
                    added_caveats: vec![],
                },
            ];
        } else if chain_nonempty {
            grant.chain.push(openspine_schemas::grant_chain::ChainStep {
                grant_id: Ulid::new(),
                parent_grant_id: Some(grant.id),
                mode: GrantMode::Live,
                selection_tokens: vec![],
                added_caveats: vec![],
            });
        }
        let pending = state.artifacts.put(b"overlay-pending").unwrap();
        state
            .store
            .insert_task_grant(&grant, &pending, OWNER_CHAT_ID)
            .unwrap();
        grant
    }

    #[tokio::test]
    async fn root_owner_export_stages_restart_required() {
        let state = test_state();
        let action = ActionId::new(EXPORT_ACTION);
        let grant = mint_grant(
            &state,
            state.owner_principal_id.to_string(),
            EXPORT_ACTION,
            None,
            false,
        );
        let payload = json!({"bundle_name": "backup-1"});
        let result = handle_overlay_export(&state, &grant, &action, OWNER_CHAT_ID, Some(&payload))
            .await
            .expect("root owner export stages");
        assert_eq!(result["restart_required"], true);
        assert_eq!(result["bundle_name"], "backup-1");
        assert_eq!(result["action"], EXPORT_ACTION);
    }

    #[tokio::test]
    async fn root_owner_restore_stages_restart_required() {
        let state = test_state();
        let action = ActionId::new(RESTORE_ACTION);
        let grant = mint_grant(
            &state,
            state.owner_principal_id.to_string(),
            RESTORE_ACTION,
            None,
            false,
        );
        let export_grant = mint_grant(
            &state,
            state.owner_principal_id.to_string(),
            EXPORT_ACTION,
            None,
            false,
        );
        let export_payload = json!({"bundle_name": "backup-1"});
        handle_overlay_export(
            &state,
            &export_grant,
            &ActionId::new(EXPORT_ACTION),
            OWNER_CHAT_ID,
            Some(&export_payload),
        )
        .await
        .expect("export handler stages");
        let pending = state
            .overlay_operations
            .process_pre_open(false, Timestamp::now())
            .unwrap()
            .unwrap();
        let meta = state
            .overlay_operations
            .begin_finalization(&pending, Timestamp::now())
            .unwrap();
        state
            .overlay_operations
            .complete_finalization(&meta)
            .unwrap();

        let payload = json!({"bundle_name": "backup-1"});
        let result = handle_overlay_restore(&state, &grant, &action, OWNER_CHAT_ID, Some(&payload))
            .await
            .expect("root owner restore stages");
        assert_eq!(result["restart_required"], true);
        assert_eq!(result["action"], RESTORE_ACTION);
    }

    #[tokio::test]
    async fn foreign_principal_is_rejected() {
        let state = test_state();
        let action = ActionId::new(EXPORT_ACTION);
        let grant = mint_grant(&state, Ulid::new().to_string(), EXPORT_ACTION, None, false);
        let payload = json!({"bundle_name": "backup-1"});
        let err = handle_overlay_export(&state, &grant, &action, OWNER_CHAT_ID, Some(&payload))
            .await
            .expect_err("foreign principal fails");
        assert!(matches!(err, DispatchError::BadRequest(msg) if msg.contains("configured owner")));
    }

    #[tokio::test]
    async fn non_root_parent_or_chain_is_rejected() {
        let state = test_state();
        let action = ActionId::new(EXPORT_ACTION);
        let parent = Ulid::new();
        let grant = mint_grant(
            &state,
            state.owner_principal_id.to_string(),
            EXPORT_ACTION,
            Some(parent),
            true,
        );
        let payload = json!({"bundle_name": "backup-1"});
        let err = handle_overlay_export(&state, &grant, &action, OWNER_CHAT_ID, Some(&payload))
            .await
            .expect_err("non-root fails");
        assert!(matches!(err, DispatchError::BadRequest(msg) if msg.contains("root grant")));
    }

    #[tokio::test]
    async fn owner_derived_worker_is_rejected() {
        let state = test_state();
        let action = ActionId::new(EXPORT_ACTION);
        let grant = mint_grant(
            &state,
            state.owner_principal_id.to_string(),
            EXPORT_ACTION,
            Some(Ulid::new()),
            true,
        );
        let payload = json!({"bundle_name": "backup-1"});
        let err = handle_overlay_export(&state, &grant, &action, OWNER_CHAT_ID, Some(&payload))
            .await
            .expect_err("worker fails");
        assert!(matches!(err, DispatchError::BadRequest(msg) if msg.contains("root grant")));
    }

    #[tokio::test]
    async fn invoked_action_must_match_handler() {
        let state = test_state();
        // Grant carries restore, but export handler is invoked with restore id
        // through the export-registered function — exact action mismatch.
        let action = ActionId::new(RESTORE_ACTION);
        let grant = mint_grant(
            &state,
            state.owner_principal_id.to_string(),
            RESTORE_ACTION,
            None,
            false,
        );
        let payload = json!({"bundle_name": "backup-1"});
        let err = handle_overlay_export(&state, &grant, &action, OWNER_CHAT_ID, Some(&payload))
            .await
            .expect_err("export handler rejects restore action id");
        assert!(matches!(err, DispatchError::BadRequest(msg) if msg.contains("export")));
    }

    #[tokio::test]
    async fn production_composed_owner_shell_grant_stages_with_other_actions() {
        let state = test_state();
        let action = ActionId::new(EXPORT_ACTION);
        let grant = handle_owner_update(&state, &owner_update("/export backup-1"))
            .await
            .expect("owner pipeline succeeds")
            .expect("owner pipeline composes a shell grant");

        assert!(grant.verify_mac(b"openspine-test-grant-hmac-key-v1"));
        assert!(grant.effectively_allows(&action));
        assert!(grant.effectively_allows(&ActionId::new(RESTORE_ACTION)));
        assert!(grant.effectively_allows(&ActionId::new("openspine.status.read")));
        assert!(state.action_catalog.is_non_delegable(&action));

        let payload = json!({"bundle_name": "backup-1"});
        let result = handle_overlay_export(&state, &grant, &action, OWNER_CHAT_ID, Some(&payload))
            .await
            .expect("production-composed multi-action grant stages export");
        assert_eq!(result["restart_required"], true);
        assert_eq!(result["action"], EXPORT_ACTION);
    }

    #[tokio::test]
    async fn malformed_and_unknown_payload_fields_fail() {
        let state = test_state();
        let action = ActionId::new(EXPORT_ACTION);
        let grant = mint_grant(
            &state,
            state.owner_principal_id.to_string(),
            EXPORT_ACTION,
            None,
            false,
        );
        for payload in [
            None,
            Some(json!({})),
            Some(json!({"bundle_name": "ok", "path": "/tmp"})),
            Some(json!({"path": "/tmp/backup"})),
            Some(json!({"bundle_name": 1})),
        ] {
            let err =
                handle_overlay_export(&state, &grant, &action, OWNER_CHAT_ID, payload.as_ref())
                    .await
                    .expect_err("malformed payload fails");
            assert!(matches!(err, DispatchError::BadRequest(_)));
        }
    }
    #[test]
    fn control_io_uses_resource_lane_while_validation_stays_bad_request() {
        let io = ControlError::Io {
            path: std::path::PathBuf::from("/control/pending-operation.json"),
            source: std::io::Error::other("disk full"),
        };
        assert!(matches!(map_control_error(io), DispatchError::Resource(_)));
        assert!(matches!(
            map_control_error(ControlError::InvalidBundleName),
            DispatchError::BadRequest(_)
        ));
    }
}
