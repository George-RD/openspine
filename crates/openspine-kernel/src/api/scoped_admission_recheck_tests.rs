//! Reachability proof (#177) for the scoped-reservation in-transaction
//! erased-counterparty recheck at
//! `store::standing_rules_scoped::consult_and_reserve_scoped_rule` (the
//! `SELECT ... FROM erased_counterparties` guard opened inside the
//! `BEGIN IMMEDIATE`).
//!
//! Census site: `store/standing_rules_scoped.rs` (in-transaction recheck).
//! Non-test caller: `api/scoped_admission::consult_scoped_rule` ->
//! `Store::consult_and_reserve_scoped_rule`, reached from the production
//! mediation entry point `api::actions::mediate_and_dispatch_action`.
//! Test entry: `mediate_and_dispatch_action` (via the shared `dispatch`
//! helper), armed with the deterministic pre-reservation seam (#177).
//!
//! The pre-transaction check in `resolve_scoped_admission`
//! (`scoped_admission.rs:219-232`) refuses first on the same identity, and
//! `Counterparty` is a required scope dimension, so the two reads can only
//! disagree if an erasure marker commits between them. The seam commits that
//! marker in exactly that window — and commits ONLY the marker, deliberately
//! leaving the matching rule active, so the in-transaction recheck is the sole
//! thing standing between a matching active rule and a stale reservation.

use jiff::Timestamp;
use openspine_schemas::action::{ActionId, GateDecision};
use rusqlite::params;
use serde_json::json;
use ulid::Ulid;

use super::scoped_admission_support::*;

/// The counterparty identity `mint_draft_grant` binds into the briefcase task
/// shape (`scoped_admission_support::mint_draft_grant`).
const BOUND_COUNTERPARTY: u128 = 11;

#[tokio::test]
async fn erased_counterparty_between_read_and_reservation_refuses_stale_reservation() {
    let env = draft_env(&["thread-1"]).await;
    // A well-formed drafts endpoint on purpose: if the recheck failed to
    // refuse, a stale reservation would proceed all the way to a provider
    // write, so the write count is a second, independent witness.
    mount_drafts(&env.api_server, 200, json!({"id": "must-not-write"})).await;
    let grant = mint_draft_grant(&env.state, "thread-1");
    let context = resolved_context(&env.state, &grant).await;
    // One active rule matching the freshly resolved context: the
    // pre-transaction check in `resolve_scoped_admission` passes because the
    // counterparty is still bound and un-erased at resolution time.
    env.state
        .store
        .activate_standing_rule(
            &scoped_manifest("rule-recheck", &context),
            None,
            Timestamp::now(),
        )
        .unwrap();

    // Arm the deterministic seam: commit ONLY the `erased_counterparties`
    // marker in the window between the pre-transaction resolution read and the
    // reservation transaction. Injecting the raw marker (rather than the full
    // `erase_counterparty`) is deliberate: `erase_counterparty` revokes every
    // rule bound to the counterparty *before* it writes the marker
    // (`store/learned_artifacts.rs`), which would make the re-SELECT find zero
    // active rules and fall back for a *different* reason — masking whether
    // the recheck fired at all. Leaving the rule active keeps this a true
    // recheck proof.
    let store = env.state.store.clone();
    env.state.arm_pre_reserve_erasure_hook(Box::new(move || {
        store.with_conn_for_test(|conn| {
            conn.execute(
                "INSERT OR IGNORE INTO erased_counterparties (counterparty_id, erased_at) \
                 VALUES (?1, ?2)",
                params![
                    Ulid::from(BOUND_COUNTERPARTY).to_string(),
                    Timestamp::now().to_string()
                ],
            )
            .expect("seam commits the erasure marker");
        });
    }));

    let (decision, budget) = dispatch(&env.state, &grant).await;

    // Pin: this test proves the in-transaction erased-counterparty recheck in
    // `Store::consult_and_reserve_scoped_rule`. Remove that recheck and the
    // matching rule — still active because the seam commits ONLY the marker and
    // never revokes it — is selected and reserved: the decision flips to Allow,
    // a `reserved` usage row appears, and the provider write fires. Every
    // assertion below then fails, so the recheck is the load-bearing guard.
    assert!(
        matches!(decision, GateDecision::ApprovalRequired { .. }),
        "an erasure committed before the reservation transaction returns the \
         action to ordinary owner approval, not Allow: {decision:?}"
    );
    assert!(
        budget.is_none(),
        "a refused consultation exposes no scoped headroom"
    );
    assert_eq!(
        usage_count(&env.state, "rule-recheck", "reserved"),
        0,
        "no stale reservation is minted against a counterparty erased mid-flight"
    );
    assert_eq!(
        usage_count(&env.state, "rule-recheck", "committed"),
        0,
        "nothing is finalized either"
    );
    assert_eq!(
        drafts_written(&env.api_server).await,
        0,
        "the recheck refuses before any provider write"
    );
    // Bind on the recheck's own audit reason so a plain no-match fallback (or
    // the pre-transaction guard) cannot masquerade as this guard firing.
    let reasons: Vec<String> = {
        let conn = env.state.store.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT event_json FROM audit_log \
                  WHERE kind = 'action.scope_context_unresolved'",
            )
            .unwrap();
        stmt.query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    assert!(
        reasons
            .iter()
            .any(|event| event.contains("counterparty was erased before scoped rule selection")),
        "the in-transaction recheck must be the arm that refuses: {reasons:?}"
    );
    // The rule was never revoked, so had the recheck not fired it would have
    // been eligible to reserve. This distinguishes the recheck from erasure's
    // own rule-revocation path.
    let rule_status: String = env
        .state
        .store
        .conn
        .lock()
        .query_row(
            "SELECT status FROM standing_rules WHERE rule_id = 'rule-recheck'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        rule_status, "active",
        "the seam leaves the rule active; only the recheck refuses"
    );
}

#[tokio::test]
async fn unarmed_seam_is_inert_and_admits_the_matching_rule() {
    // Control: with the seam unarmed, the identical setup admits the rule and
    // reserves budget. This proves the refusal above is caused by the injected
    // marker, not by an incidental setup defect, and that the seam is a no-op
    // on the normal path.
    let env = draft_env(&["thread-1"]).await;
    mount_drafts(&env.api_server, 200, json!({"id": "draft-ok"})).await;
    let grant = mint_draft_grant(&env.state, "thread-1");
    let context = resolved_context(&env.state, &grant).await;
    env.state
        .store
        .activate_standing_rule(
            &scoped_manifest("rule-control", &context),
            None,
            Timestamp::now(),
        )
        .unwrap();

    let result = crate::api::actions::mediate_and_dispatch_action(
        &env.state,
        &grant,
        ActionId::new("email.create_draft"),
        &crate::test_support::telegram_surface(CHAT_ID),
        Some(&draft_payload()),
        crate::api::actions::FailureSurface::DirectResponse,
        None,
    )
    .await
    .expect("a matching rule with headroom admits without a transport error");

    assert!(
        matches!(result.0, GateDecision::Allow),
        "an unarmed seam leaves the matching rule to admit: {:?}",
        result.0
    );
    // A successful execution finalizes the reservation to `committed` and
    // writes the draft, so the admission genuinely spent budget — the exact
    // outcome the armed test suppresses.
    assert_eq!(
        usage_count(&env.state, "rule-control", "committed"),
        1,
        "the admitted rule finalizes exactly one unit of budget"
    );
    assert_eq!(
        usage_count(&env.state, "rule-control", "reserved"),
        0,
        "a finalized reservation no longer sits in the reserved state"
    );
    assert_eq!(
        drafts_written(&env.api_server).await,
        1,
        "the unarmed path proceeds all the way to a provider write"
    );
}
