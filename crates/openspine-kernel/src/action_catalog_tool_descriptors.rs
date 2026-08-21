//! Curated, kernel-owned tool descriptors for every currently-dispatchable
//! action id (spec #209 IT1). Mirrors the curated-const philosophy of
//! `action_catalog.rs` and the sibling `action_catalog_data.rs`: each entry is
//! reviewed metadata, never derived from a fixture or from connector-supplied
//! data.
//!
//! Scope rule: exactly the ids the kernel actually mediates through
//! `POST /v1/actions` (the `ActionHandlerRegistry::default_registrations()`
//! set). Intentionally-unwired PRD ids (`route.activate`, `workflow.activate`,
//! `capability_pack.change`, `policy.change_proposal`, `connector.enable`) are
//! not dispatchable and carry no descriptor; the completeness gate test is
//! scoped to the dispatchable set.
//!
//! Invariants the gate test pins:
//! - one descriptor per dispatchable id (cardinality == 16);
//! - `selection_token_required` mirrors
//!   `ActionCatalog::requires_selection_token(id).is_some()` exactly.
//!
//! `approval_required` is a curated *presentation* flag set by reviewed
//! judgment per action (not derived from any catalog axis); each `true` value
//! carries a one-line rationale.

use openspine_schemas::action::{ActionId, ToolDescriptor};
use serde_json::json;

fn id(s: &str) -> ActionId {
    ActionId::new(s)
}

/// A single-string-field payload schema (the common shape for the reply and
/// bundle actions), with unknown fields rejected to mirror the handlers'
/// `#[serde(deny_unknown_fields)]` payload contracts.
fn single_string_field(field: &str, description: &str) -> serde_json::Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [field],
        "properties": { field: { "type": "string", "description": description } }
    })
}

/// One descriptor per currently-dispatchable action id. The set is asserted
/// complete (and its cardinality pinned) against the handler registry by
/// `action_catalog_tests.rs`.
pub(super) fn tool_descriptors() -> Vec<(ActionId, ToolDescriptor)> {
    vec![
        (
            id("openspine.status.read"),
            ToolDescriptor {
                name: "read_status".to_string(),
                description: "Read the kernel's current status.".to_string(),
                parameters_schema: json!({
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {}
                }),
                approval_required: false,
                selection_token_required: false,
            },
        ),
        (
            id("telegram.reply:owner_channel"),
            ToolDescriptor {
                name: "reply_owner_telegram".to_string(),
                description: "Send a text reply to the owner on their Telegram channel."
                    .to_string(),
                parameters_schema: single_string_field("text", "The reply text to send."),
                approval_required: false,
                selection_token_required: false,
            },
        ),
        (
            id("terminal.reply:owner_device"),
            ToolDescriptor {
                name: "reply_owner_terminal".to_string(),
                description: "Send a text reply to the owner on their active terminal device."
                    .to_string(),
                parameters_schema: single_string_field("text", "The reply text to send."),
                approval_required: false,
                selection_token_required: false,
            },
        ),
        (
            id("email.read_thread:selected_no_attachments"),
            ToolDescriptor {
                name: "read_selected_email_thread".to_string(),
                description: "Read the owner-selected email thread (no attachments), consuming a \
                              grant-bound selection token."
                    .to_string(),
                parameters_schema: single_string_field(
                    "selection_token_id",
                    "The id of the grant-bound selection token authorizing this read.",
                ),
                approval_required: false,
                // Mirrors the catalog's sole token-requiring dispatchable id.
                selection_token_required: true,
            },
        ),
        (
            id("lyra.ui.preview"),
            ToolDescriptor {
                name: "preview_owner_ui".to_string(),
                description: "Preview a subject/body draft to the owner before any send."
                    .to_string(),
                parameters_schema: json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["subject", "body"],
                    "properties": {
                        "subject": { "type": "string", "description": "The preview subject." },
                        "body": { "type": "string", "description": "The preview body." }
                    }
                }),
                approval_required: false,
                selection_token_required: false,
            },
        ),
        (
            id("artifact.propose"),
            ToolDescriptor {
                name: "propose_artifact".to_string(),
                description: "Propose a governed artifact (route/agent/workflow/pack/policy/\
                              model_swap/standing_rule/persona) as YAML for owner approval."
                    .to_string(),
                parameters_schema: json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["kind", "yaml"],
                    "properties": {
                        "kind": {
                            "type": "string",
                            "enum": [
                                "route", "agent", "workflow", "pack", "policy",
                                "model_swap", "standing_rule", "persona"
                            ],
                            "description": "The proposable artifact kind."
                        },
                        "yaml": {
                            "type": "string",
                            "description": "The artifact document as YAML, lifecycle_state: proposed."
                        }
                    }
                }),
                approval_required: false,
                selection_token_required: false,
            },
        ),
        (
            id("artifact.revoke"),
            ToolDescriptor {
                name: "revoke_standing_rule".to_string(),
                description: "Revoke a standing rule by id, narrowing standing authority."
                    .to_string(),
                parameters_schema: single_string_field(
                    "rule_id",
                    "The id of the standing rule to revoke.",
                ),
                approval_required: false,
                selection_token_required: false,
            },
        ),
        (
            id("plan.propose"),
            ToolDescriptor {
                name: "propose_plan".to_string(),
                description: "Propose a Plan to the owner for preview and approval.".to_string(),
                parameters_schema: json!({
                    "type": "object",
                    "description": "A Plan document (see openspine_schemas::plan::Plan).",
                    "additionalProperties": true
                }),
                approval_required: false,
                selection_token_required: false,
            },
        ),
        (
            id("artifact.nominate_upstream"),
            ToolDescriptor {
                name: "nominate_artifact_upstream".to_string(),
                description: "Nominate a compatible, depersonalized learned artifact as an \
                              upstream candidate."
                    .to_string(),
                parameters_schema: json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["kind", "artifact_id", "version", "depersonalized"],
                    "properties": {
                        "kind": { "type": "string", "description": "The proposable artifact kind." },
                        "artifact_id": { "type": "string", "description": "The learned artifact id." },
                        "version": {
                            "type": "integer",
                            "minimum": 0,
                            "description": "The learned artifact version."
                        },
                        "depersonalized": {
                            "type": "boolean",
                            "description": "Must be true; asserts the artifact carries no owner PII."
                        }
                    }
                }),
                approval_required: false,
                selection_token_required: false,
            },
        ),
        (
            id("worker.commission"),
            ToolDescriptor {
                name: "commission_worker".to_string(),
                description: "Commission an attenuated sub-worker with a bounded allowed-action \
                              set and expiry."
                    .to_string(),
                parameters_schema: json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": [
                        "agent_id", "allowed_actions", "expires_before", "purpose",
                        "route_id", "workflow_id", "capability_pack_id", "receipt"
                    ],
                    "properties": {
                        "agent_id": { "type": "string" },
                        "allowed_actions": { "type": "array", "items": { "type": "string" } },
                        "bound_parameters": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["name", "value"],
                                "properties": {
                                    "name": { "type": "string" },
                                    "value": { "type": "string" }
                                }
                            }
                        },
                        "expires_before": { "type": "string", "description": "RFC 3339 timestamp." },
                        "purpose": { "type": "string" },
                        "route_id": { "type": "string" },
                        "workflow_id": { "type": "string" },
                        "capability_pack_id": { "type": "string" },
                        "counterparty_channel": { "type": ["string", "null"] },
                        "counterparty_identifier": { "type": ["string", "null"] },
                        "receipt": {
                            "type": "string",
                            "description": "Caller-generated idempotency receipt (ULID)."
                        }
                    }
                }),
                // Commissioning mints a fresh, attenuated delegated grant (a new
                // actor with authority): reviewed judgment flags it so the owner
                // ratifies creating delegated actors.
                approval_required: true,
                selection_token_required: false,
            },
        ),
        (
            id("worker.report_result"),
            ToolDescriptor {
                name: "report_worker_result".to_string(),
                description: "Report a commissioned worker's outcome, offered slots, and requests."
                    .to_string(),
                parameters_schema: json!({
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "outcome": {
                            "type": "string",
                            "enum": ["completed", "failed", "awaiting"]
                        },
                        "offered_slots": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["id", "label"],
                                "properties": {
                                    "id": { "type": "string" },
                                    "label": { "type": "string" }
                                }
                            }
                        },
                        "requests": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["kind"],
                                "properties": {
                                    "kind": { "type": "string" },
                                    "detail_ref": { "type": ["object", "null"] }
                                }
                            }
                        },
                        "notes_ref": { "type": ["object", "null"] }
                    }
                }),
                approval_required: false,
                selection_token_required: false,
            },
        ),
        (
            id("worker.failed"),
            ToolDescriptor {
                name: "report_worker_failed".to_string(),
                description: "Report that a commissioned worker failed, with a typed reason."
                    .to_string(),
                parameters_schema: json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["reason"],
                    "properties": {
                        "reason": {
                            "type": "string",
                            "enum": ["shell_exited", "crash", "timeout", "lost", "startup_failure"]
                        },
                        "detail_ref": { "type": ["object", "null"] }
                    }
                }),
                approval_required: false,
                selection_token_required: false,
            },
        ),
        (
            id("skill.context"),
            ToolDescriptor {
                name: "read_skill_context".to_string(),
                description: "Return installed, approved-shelf skill bodies for the grant's task \
                              class as untrusted competence data."
                    .to_string(),
                // Task class is derived from the authenticated grant purpose;
                // the caller supplies no parameters.
                parameters_schema: json!({
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {}
                }),
                approval_required: false,
                selection_token_required: false,
            },
        ),
        (
            id("openspine.overlay.export"),
            ToolDescriptor {
                name: "export_overlay".to_string(),
                description: "Stage an export of a named overlay bundle (root owner grant only)."
                    .to_string(),
                parameters_schema: single_string_field(
                    "bundle_name",
                    "The overlay bundle name to export.",
                ),
                approval_required: false,
                selection_token_required: false,
            },
        ),
        (
            id("openspine.overlay.restore"),
            ToolDescriptor {
                name: "restore_overlay".to_string(),
                description: "Restore a named overlay bundle into the live governed set (root \
                              owner grant only)."
                    .to_string(),
                parameters_schema: single_string_field(
                    "bundle_name",
                    "The overlay bundle name to restore.",
                ),
                // Restore overwrites the live governed artifact set: a
                // privileged mutation the owner should ratify.
                approval_required: true,
                selection_token_required: false,
            },
        ),
        (
            id("openspine.counterparty.erase"),
            ToolDescriptor {
                name: "erase_counterparty".to_string(),
                description: "Crypto-erase a counterparty by id: sign a terminal-ledger entry, \
                              then invalidate derived artifacts and delete the payload key (root \
                              owner grant only, irreversible)."
                    .to_string(),
                parameters_schema: single_string_field(
                    "counterparty_id",
                    "The ULID of the counterparty to crypto-erase.",
                ),
                // Irreversible destruction of a counterparty's payload key and
                // derived artifacts: a privileged mutation the owner should ratify.
                approval_required: true,
                selection_token_required: false,
            },
        ),
    ]
}
