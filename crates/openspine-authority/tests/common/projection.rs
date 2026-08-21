//! Fixture builders for the `project_catalog` projection tests (spec #209,
//! IT2). Split out of `common/mod.rs` to keep each file under the
//! 500-line-per-file gate; re-exported through `common` so the projection
//! tests reach them as `common::*`.

use openspine_schemas::action::{ActionCatalog, ActionId, ToolDescriptor};
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::grant::{GrantLimits, GrantMode, TaskGrant};
use ulid::Ulid;

/// A minimal, fully-populated `TaskGrant` with caller-controlled action lists,
/// for the `project_catalog` projection tests. The projection is a pure
/// function of `(grant, catalog)`, so the grant needs no sealing, no MAC, and
/// no lineage — every other field is a fixed, inert test value. Mirrors the
/// literal builders in the kernel (`store/tests.rs::sample_grant`).
pub fn projection_grant(
    allowed: Vec<ActionId>,
    approval_required: Vec<ActionId>,
    denied: Vec<ActionId>,
) -> TaskGrant {
    let issued_at = jiff::Timestamp::now();
    let id = Ulid::new();
    TaskGrant {
        id,
        schema_version: 1,
        lifecycle_state: Lifecycle::Active,
        user: Ulid::new().into(),
        purpose: "projection-test".to_string(),
        issued_by: "kernel".to_string(),
        issued_at,
        expires_at: issued_at + std::time::Duration::from_secs(120),
        event_id: Ulid::new(),
        route_id: "owner_telegram_main_assistant".to_string(),
        agent_id: "main_assistant_agent".to_string(),
        workflow_id: "owner_control_conversation".to_string(),
        capability_pack_id: "owner_control_basic_pack".to_string(),
        authority_sources: vec![],
        selection_tokens: vec![],
        allowed_actions: allowed,
        approval_required_actions: approval_required,
        denied_actions: denied,
        allowed_egress_classes: vec![],
        output_channels: vec![],
        limits: GrantLimits {
            max_model_calls: 8,
            max_artifacts: 20,
            max_runtime_seconds: 120,
        },
        task_token: "projection-test-token".to_string(),
        root_grant_id: id,
        parent_grant_id: None,
        mode: GrantMode::Live,
        chain: vec![],
        caveat_mac: String::new(),
        thread_id: None,
        persona_id: None,
    }
}

/// One catalog-owned tool descriptor with a trivial, unknown-field-rejecting
/// parameter schema. `name` doubles as the model-facing name and the schema's
/// description, so tests can assert on it.
pub fn tool_descriptor(
    name: &str,
    approval_required: bool,
    selection_token_required: bool,
) -> ToolDescriptor {
    ToolDescriptor {
        name: name.to_string(),
        description: format!("{name} tool description"),
        parameters_schema: serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {}
        }),
        approval_required,
        selection_token_required,
    }
}

/// The catalog the projection tests consult. It carries descriptors for a
/// controlled set of ids — including the privileged `email.send` and
/// `worker.commission` so the structural-absence test proves absence is *by
/// construction* (the grant never lists them), not merely because a descriptor
/// is missing. `undescribed.action` deliberately has an id but no descriptor,
/// for the omission test.
pub fn projection_catalog() -> ActionCatalog {
    let entries = [
        (
            "openspine.status.read",
            tool_descriptor("read_status", false, false),
        ),
        (
            "telegram.reply:owner_channel",
            tool_descriptor("reply_owner", false, false),
        ),
        (
            "connector.enable",
            tool_descriptor("enable_connector", true, false),
        ),
        (
            "artifact.activate",
            tool_descriptor("activate_artifact", true, true),
        ),
        ("email.send", tool_descriptor("send_email", true, false)),
        (
            "worker.commission",
            tool_descriptor("commission_worker", false, false),
        ),
    ];
    let ids: Vec<ActionId> = entries
        .iter()
        .map(|(id, _)| ActionId::new(*id))
        .chain(std::iter::once(ActionId::new("undescribed.action")))
        .collect();
    ActionCatalog::new(ids).with_tool_descriptors(
        entries
            .into_iter()
            .map(|(id, descriptor)| (ActionId::new(id), descriptor)),
    )
}
