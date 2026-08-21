//! Tests for the root-owner counterparty-erasure origination handler (#172).
//!
//! `owner_command_runs_generic_reviewed_scope_sweep_via_caller_path` discharges
//! the #130 inherited obligation the store-level census left open: it drives the
//! GENERIC reviewed-scope standing-rule sweep (the #176 branch of
//! `store::learned_artifacts::mark_learned_artifacts_erased` that revokes a rule
//! by its reviewed-scope `Counterparty` binding, with NO matching
//! learned-artifact provenance row) through the new command handler. A second
//! rule bound to a DIFFERENT counterparty is the control: it must survive, so an
//! over-broad revocation fails the test.
//!
//! The scoped-reservation *transactional recheck* is deliberately NOT re-proved
//! here. The pre-transaction `is_counterparty_erased` guard in
//! `resolve_scoped_admission` refuses first once a counterparty is erased, so a
//! handler-first dispatch never reaches the reservation transaction. Its sole
//! valid proof stays the #177 seam test
//! `api::scoped_admission::recheck_tests::
//! erased_counterparty_between_read_and_reservation_refuses_stale_reservation`,
//! carried forward unchanged.

use jiff::Timestamp;
use rusqlite::{params, OptionalExtension};
use serde_json::json;
use ulid::Ulid;

use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::grant::{GrantLimits, GrantMode, TaskGrant};

use super::handle_erase_counterparty;
use crate::api::actions::DispatchError;
use crate::api::dispatch_tests::OWNER_CHAT_ID;
use crate::api::scoped_admission::scoped_admission_support::{
    draft_env, mint_draft_grant, mint_draft_grant_with_counterparty, resolved_context,
    scoped_manifest,
};
use crate::pipeline::AppState;
use crate::test_support::fixtures::test_state;

const ERASE_ACTION: &str = "openspine.counterparty.erase";
/// The counterparty identity `mint_draft_grant` binds into its briefcase.
const TARGET_COUNTERPARTY: u128 = 11;
/// The control counterparty; its reviewed-scope rule must survive the erase.
const CONTROL_COUNTERPARTY: u128 = 22;

/// Mint a counterparty-erasure grant. `parent`/`chain_nonempty` mirror the
/// overlay export/restore test helper so the negative cases can forge non-root
/// and owner-derived-worker grants exactly the same way.
fn mint_grant(
    state: &AppState,
    user: openspine_schemas::ids::PrincipalId,
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
    let pending = state.artifacts.put(b"erase-pending").unwrap();
    state
        .store
        .insert_task_grant(
            &grant,
            &pending,
            &crate::test_support::owner_surface_for(state, OWNER_CHAT_ID),
        )
        .unwrap();
    grant
}

fn root_owner_grant(state: &AppState) -> TaskGrant {
    mint_grant(state, state.owner.principal_id, ERASE_ACTION, None, false)
}

fn rule_status(state: &AppState, rule_id: &str) -> Option<String> {
    state.store.with_conn_for_test(|conn| {
        conn.query_row(
            "SELECT status FROM standing_rules WHERE rule_id = ?1",
            params![rule_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .unwrap()
    })
}

#[tokio::test]
async fn owner_command_runs_generic_reviewed_scope_sweep_via_caller_path() {
    let env = draft_env(&["thread-1", "thread-2"]).await;
    env.state
        .overlay_operations
        .initialize_terminal_ledger()
        .expect("terminal ledger initializes for the test data root");
    let now = Timestamp::now();

    // Target rule: reviewed scope binds TARGET_COUNTERPARTY (mint_draft_grant's
    // default). No learned-artifact row is seeded, so only the generic
    // reviewed-scope sweep can revoke it.
    let target_grant = mint_draft_grant(&env.state, "thread-1");
    let target_ctx = resolved_context(&env.state, &target_grant).await;
    env.state
        .store
        .activate_standing_rule(&scoped_manifest("rule-target", &target_ctx), None, now)
        .expect("target scoped rule activates");

    // Control rule: a DIFFERENT counterparty reviewed scope that must survive an
    // over-broad revocation.
    let control_grant = mint_draft_grant_with_counterparty(
        &env.state,
        "thread-2",
        Ulid::from(CONTROL_COUNTERPARTY),
    );
    let control_ctx = resolved_context(&env.state, &control_grant).await;
    env.state
        .store
        .activate_standing_rule(&scoped_manifest("rule-control", &control_ctx), None, now)
        .expect("control scoped rule activates");

    assert_eq!(
        rule_status(&env.state, "rule-target").as_deref(),
        Some("active")
    );
    assert_eq!(
        rule_status(&env.state, "rule-control").as_deref(),
        Some("active")
    );

    // Originate the erasure through the owner command with a true root grant.
    let grant = root_owner_grant(&env.state);
    let action = ActionId::new(ERASE_ACTION);
    let payload = json!({"counterparty_id": Ulid::from(TARGET_COUNTERPARTY).to_string()});
    let result = handle_erase_counterparty(
        &env.state,
        &grant,
        &action,
        &crate::test_support::owner_surface_for(&env.state, OWNER_CHAT_ID),
        Some(&payload),
    )
    .await
    .expect("root owner erasure command succeeds");

    assert_eq!(
        result["counterparty_id"],
        Ulid::from(TARGET_COUNTERPARTY).to_string()
    );
    // The rule carries no learned-artifact provenance row, so the provenance
    // path invalidates nothing; the generic reviewed-scope sweep still revokes.
    assert_eq!(result["derived_artifacts_invalidated"], 0);
    // First erasure recorded against a freshly initialized ledger (sequence 0).
    assert_eq!(result["ledger_sequence"], 1);
    // A non-sensitive shape only: never the invalidated learned-artifact ids.
    assert!(result.get("invalidated_identities").is_none());

    // The generic sweep revoked the target rule; the control rule survives.
    assert_eq!(
        rule_status(&env.state, "rule-target").as_deref(),
        Some("revoked"),
        "the reviewed-scope-bound target rule must be revoked by the generic sweep"
    );
    assert_eq!(
        rule_status(&env.state, "rule-control").as_deref(),
        Some("active"),
        "a rule bound to a different counterparty must not be swept"
    );

    // Targeting is exact and durable.
    assert!(env
        .state
        .store
        .is_counterparty_erased(Ulid::from(TARGET_COUNTERPARTY))
        .unwrap());
    assert!(!env
        .state
        .store
        .is_counterparty_erased(Ulid::from(CONTROL_COUNTERPARTY))
        .unwrap());

    // The hash chain still verifies and exactly one erase event was appended.
    assert!(env.state.store.verify_audit_chain().unwrap());
    assert_eq!(
        env.state
            .store
            .count_audit_events_of_kind("counterparty.erased")
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn foreign_principal_is_rejected() {
    let state = test_state();
    let action = ActionId::new(ERASE_ACTION);
    let grant = mint_grant(&state, Ulid::new().into(), ERASE_ACTION, None, false);
    let payload = json!({"counterparty_id": Ulid::new().to_string()});
    let err = handle_erase_counterparty(
        &state,
        &grant,
        &action,
        &crate::test_support::owner_surface_for(&state, OWNER_CHAT_ID),
        Some(&payload),
    )
    .await
    .expect_err("foreign principal fails");
    assert!(matches!(err, DispatchError::BadRequest(msg) if msg.contains("configured owner")));
}

#[tokio::test]
async fn non_root_or_delegated_hop_grant_is_rejected() {
    let state = test_state();
    let action = ActionId::new(ERASE_ACTION);
    let parent = Ulid::new();
    let grant = mint_grant(
        &state,
        state.owner.principal_id,
        ERASE_ACTION,
        Some(parent),
        true,
    );
    let payload = json!({"counterparty_id": Ulid::new().to_string()});
    let err = handle_erase_counterparty(
        &state,
        &grant,
        &action,
        &crate::test_support::owner_surface_for(&state, OWNER_CHAT_ID),
        Some(&payload),
    )
    .await
    .expect_err("non-root fails");
    assert!(matches!(err, DispatchError::BadRequest(msg) if msg.contains("root grant")));
}

#[tokio::test]
async fn owner_derived_worker_grant_is_rejected() {
    let state = test_state();
    let action = ActionId::new(ERASE_ACTION);
    let grant = mint_grant(
        &state,
        state.owner.principal_id,
        ERASE_ACTION,
        Some(Ulid::new()),
        true,
    );
    let payload = json!({"counterparty_id": Ulid::new().to_string()});
    let err = handle_erase_counterparty(
        &state,
        &grant,
        &action,
        &crate::test_support::owner_surface_for(&state, OWNER_CHAT_ID),
        Some(&payload),
    )
    .await
    .expect_err("worker grant fails");
    assert!(matches!(err, DispatchError::BadRequest(msg) if msg.contains("root grant")));
}

#[tokio::test]
async fn wrong_action_id_is_rejected() {
    let state = test_state();
    let grant = root_owner_grant(&state);
    // The erase handler invoked with a different action id.
    let action = ActionId::new("openspine.overlay.export");
    let payload = json!({"counterparty_id": Ulid::new().to_string()});
    let err = handle_erase_counterparty(
        &state,
        &grant,
        &action,
        &crate::test_support::owner_surface_for(&state, OWNER_CHAT_ID),
        Some(&payload),
    )
    .await
    .expect_err("wrong action id fails");
    assert!(
        matches!(err, DispatchError::BadRequest(msg) if msg.contains("openspine.counterparty.erase only"))
    );
}

#[tokio::test]
async fn malformed_and_unknown_payload_fields_fail() {
    let state = test_state();
    let action = ActionId::new(ERASE_ACTION);
    let grant = root_owner_grant(&state);
    for payload in [
        None,
        Some(json!({})),
        Some(json!({"counterparty_id": Ulid::new().to_string(), "extra": 1})),
        Some(json!({"other": "x"})),
        Some(json!({"counterparty_id": 1})),
    ] {
        let err = handle_erase_counterparty(
            &state,
            &grant,
            &action,
            &crate::test_support::owner_surface_for(&state, OWNER_CHAT_ID),
            payload.as_ref(),
        )
        .await
        .expect_err("malformed payload fails");
        assert!(matches!(err, DispatchError::BadRequest(_)));
    }
}

#[tokio::test]
async fn invalid_ulid_is_rejected() {
    let state = test_state();
    let action = ActionId::new(ERASE_ACTION);
    let grant = root_owner_grant(&state);
    let payload = json!({"counterparty_id": "not-a-ulid"});
    let err = handle_erase_counterparty(
        &state,
        &grant,
        &action,
        &crate::test_support::owner_surface_for(&state, OWNER_CHAT_ID),
        Some(&payload),
    )
    .await
    .expect_err("invalid ulid fails");
    assert!(matches!(err, DispatchError::BadRequest(msg) if msg.contains("not a valid id")));
}

#[tokio::test]
async fn system_scope_is_rejected() {
    let state = test_state();
    let action = ActionId::new(ERASE_ACTION);
    let grant = root_owner_grant(&state);
    let payload = json!({"counterparty_id": Ulid::nil().to_string()});
    let err = handle_erase_counterparty(
        &state,
        &grant,
        &action,
        &crate::test_support::owner_surface_for(&state, OWNER_CHAT_ID),
        Some(&payload),
    )
    .await
    .expect_err("SYSTEM_SCOPE fails");
    assert!(matches!(err, DispatchError::BadRequest(msg) if msg.contains("SYSTEM_SCOPE")));
}
