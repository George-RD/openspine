//! Selection and matching acceptance tests for scope-matched standing-rule
//! admission (#128): exactly-one matching, disjoint coexistence with
//! independent budgets, and the fail-closed zero/ambiguous outcomes. Every
//! test drives a real `email.create_draft` request through the production
//! mediation path against a mocked Gmail provider.

use jiff::Timestamp;
use openspine_schemas::action::GateDecision;
use openspine_schemas::digest::canonical_json;
use openspine_schemas::standing_rule::BudgetWindow;
use serde_json::json;
use ulid::Ulid;

use super::scoped_admission_support::*;
use crate::store::standing_rules_tests::manifest;

/// Acceptance: a resolved, digest-bound context reaches the shared
/// `gmail.create_draft` executor from scope-matched admission — the third
/// caller — and `Executed` finalizes the reservation.
#[tokio::test]
async fn scoped_rule_admits_draft_through_production_path_and_finalizes_reservation() {
    let env = draft_env(&["thread-1"]).await;
    mount_drafts(&env.api_server, 200, json!({"id": "draft-1"})).await;
    let grant = mint_draft_grant(&env.state, "thread-1");
    let context = resolved_context(&env.state, &grant).await;
    env.state
        .store
        .activate_standing_rule(
            &scoped_manifest("rule-scoped", &context),
            None,
            Timestamp::now(),
        )
        .unwrap();

    let (decision, quota_remaining) = dispatch(&env.state, &grant).await;

    assert!(
        matches!(decision, GateDecision::Allow),
        "a matching reviewed scope admits without a fresh owner approval"
    );
    assert_eq!(
        drafts_written(&env.api_server).await,
        1,
        "the shared executor performed exactly one provider write"
    );
    assert_eq!(usage_count(&env.state, "rule-scoped", "committed"), 1);
    assert_eq!(usage_count(&env.state, "rule-scoped", "reserved"), 0);
    assert_eq!(
        env.state.store.count_pending_draft_writes().unwrap(),
        0,
        "a confirmed write resolves the reconciliation fence"
    );
    assert_eq!(
        quota_remaining,
        Some(4),
        "headroom is returned on an authorized Allow"
    );
    assert!(audit_count(&env.state, "draft.created") >= 1);
    // Audit boundary: the admitting rule id, its version, and BOTH digests are
    // recorded with the admission, so an auditor can reconstruct which
    // reviewed responsibility spent which budget.
    let gated = env
        .state
        .store
        .all_audit_event_jsons()
        .unwrap()
        .into_iter()
        .find(|raw| raw.contains("scoped standing-rule effective Allow admitted before effect"))
        .expect("the scoped admission writes an action.gated audit row");
    assert!(
        gated.contains("rule-scoped"),
        "admitting rule id is recorded"
    );
    assert!(gated.contains("v1"), "admitting rule version is recorded");
    assert!(
        gated.contains(context.compatibility_digest().as_str()),
        "the bound compatibility epoch is recorded with the admission"
    );
    assert!(
        gated.contains(context.reviewed_scope_digest().expect("scope key").as_str()),
        "the reviewed scope digest is recorded with the admission"
    );
}

/// Acceptance: two different targets cannot form one pattern — a rule
/// reviewed for one thread never admits another, and a scope mismatch
/// consumes no budget and schedules nothing.
#[tokio::test]
async fn two_different_targets_cannot_form_one_pattern() {
    let env = draft_env(&["thread-1", "thread-2"]).await;
    mount_drafts(&env.api_server, 200, json!({"id": "draft-1"})).await;
    let reviewed_grant = mint_draft_grant(&env.state, "thread-2");
    let reviewed = resolved_context(&env.state, &reviewed_grant).await;
    let other_grant = mint_draft_grant(&env.state, "thread-1");
    let other = resolved_context(&env.state, &other_grant).await;
    assert_ne!(
        reviewed.reviewed_scope_digest(),
        other.reviewed_scope_digest(),
        "a different bound target is a different reviewed scope"
    );
    assert_eq!(
        reviewed.compatibility_digest(),
        other.compatibility_digest(),
        "the drift epoch covers declaration axes only, so it cannot be the scope key"
    );
    env.state
        .store
        .activate_standing_rule(
            &scoped_manifest("rule-thread-2", &reviewed),
            None,
            Timestamp::now(),
        )
        .unwrap();

    let (decision, budget) = dispatch(&env.state, &other_grant).await;

    assert!(
        matches!(decision, GateDecision::ApprovalRequired { .. }),
        "a scope mismatch returns the action to ordinary owner approval"
    );
    assert!(budget.is_none(), "a denial exposes no headroom");
    assert_eq!(drafts_written(&env.api_server).await, 0);
    assert_eq!(usage_count(&env.state, "rule-thread-2", "reserved"), 0);
    assert_eq!(usage_count(&env.state, "rule-thread-2", "committed"), 0);
    assert_eq!(scheduled_timer_count(&env.state), 0);
}

/// Acceptance: two different accounts cannot form one pattern. The account
/// identity is kernel-resolved from the configured mailbox, so a rule
/// reviewed on one account never admits the other — and the kernel never
/// remaps the rule onto the successor account.
#[tokio::test]
async fn two_different_accounts_cannot_form_one_pattern() {
    let reviewed_env = draft_env_with_mailbox("first-owner@example.com", &["thread-1"]).await;
    let reviewed_grant = mint_draft_grant(&reviewed_env.state, "thread-1");
    let reviewed = resolved_context(&reviewed_env.state, &reviewed_grant).await;

    let env = draft_env_with_mailbox("second-owner@example.com", &["thread-1"]).await;
    mount_drafts(&env.api_server, 200, json!({"id": "draft-1"})).await;
    let grant = mint_draft_grant(&env.state, "thread-1");
    let resolved_here = resolved_context(&env.state, &grant).await;
    assert_ne!(
        reviewed.reviewed_scope_digest(),
        resolved_here.reviewed_scope_digest(),
        "a different account identity is a different reviewed scope"
    );
    env.state
        .store
        .activate_standing_rule(
            &scoped_manifest("rule-first-account", &reviewed),
            None,
            Timestamp::now(),
        )
        .unwrap();

    let (decision, _) = dispatch(&env.state, &grant).await;

    assert!(matches!(decision, GateDecision::ApprovalRequired { .. }));
    assert_eq!(drafts_written(&env.api_server).await, 0);
    assert_eq!(usage_count(&env.state, "rule-first-account", "reserved"), 0);
}

/// Acceptance: two disjoint scoped rules coexist for one action, each
/// matching only its own context, each holding its own budget with no
/// pooling between them.
#[tokio::test]
async fn disjoint_scoped_rules_coexist_with_independent_budgets() {
    let env = draft_env(&["thread-1", "thread-2"]).await;
    mount_drafts(&env.api_server, 200, json!({"id": "draft-1"})).await;
    let grant_one = mint_draft_grant(&env.state, "thread-1");
    let grant_two = mint_draft_grant(&env.state, "thread-2");
    let context_one = resolved_context(&env.state, &grant_one).await;
    let context_two = resolved_context(&env.state, &grant_two).await;
    let now = Timestamp::now();
    env.state
        .store
        .activate_standing_rule(&scoped_manifest("rule-one", &context_one), None, now)
        .unwrap();
    env.state
        .store
        .activate_standing_rule(&scoped_manifest("rule-two", &context_two), None, now)
        .unwrap();

    let (first, _) = dispatch(&env.state, &grant_one).await;
    assert!(matches!(first, GateDecision::Allow));
    assert_eq!(usage_count(&env.state, "rule-one", "committed"), 1);
    assert_eq!(
        usage_count(&env.state, "rule-two", "committed"),
        0,
        "budgets are strictly per rule; there is no aggregate per-action counter"
    );

    let (second, _) = dispatch(&env.state, &grant_two).await;
    assert!(matches!(second, GateDecision::Allow));
    assert_eq!(usage_count(&env.state, "rule-one", "committed"), 1);
    assert_eq!(usage_count(&env.state, "rule-two", "committed"), 1);
    assert_eq!(drafts_written(&env.api_server).await, 2);
}

/// Acceptance: an ambiguous overlap fails closed — no reservation row, no
/// scheduled timer, no budget consumed, and durable owner-actionable
/// evidence. There is no tie-break by recency, narrowness, or ordering.
#[tokio::test]
async fn ambiguous_overlap_fails_closed_and_consumes_no_budget() {
    let env = draft_env(&["thread-1"]).await;
    mount_drafts(&env.api_server, 200, json!({"id": "draft-1"})).await;
    let grant = mint_draft_grant(&env.state, "thread-1");
    let context = resolved_context(&env.state, &grant).await;
    let now = Timestamp::now();
    env.state
        .store
        .activate_standing_rule(&scoped_manifest("rule-overlap-a", &context), None, now)
        .unwrap();
    env.state
        .store
        .activate_standing_rule(&scoped_manifest("rule-overlap-b", &context), None, now)
        .unwrap();
    // Activating a second rule over the *same* reviewed scope revokes the
    // first, so ordinary activation cannot produce an overlap. Reinstate the
    // first row directly to model the states that can — a restored overlay, a
    // migrated legacy row, or a racing activation — and prove the matcher
    // still refuses rather than tie-breaking.
    env.state
        .store
        .conn
        .lock()
        .execute(
            "UPDATE standing_rules SET status = 'active', revoked_at = NULL \
             WHERE rule_id = 'rule-overlap-a'",
            [],
        )
        .unwrap();

    let (decision, budget) = dispatch(&env.state, &grant).await;

    assert!(
        matches!(decision, GateDecision::ApprovalRequired { .. }),
        "an ambiguous overlap falls back to ordinary owner approval"
    );
    assert!(budget.is_none());
    assert_eq!(drafts_written(&env.api_server).await, 0);
    for rule in ["rule-overlap-a", "rule-overlap-b"] {
        assert_eq!(usage_count(&env.state, rule, "reserved"), 0);
        assert_eq!(usage_count(&env.state, rule, "committed"), 0);
    }
    assert_eq!(scheduled_timer_count(&env.state), 0);
    assert_eq!(
        audit_count(&env.state, "standing_rule.ambiguous_scope_overlap"),
        1,
        "the refusal leaves durable owner-actionable evidence"
    );
    // Pin the ordering directly. `cancel_standing_rule_reservation` DELETEs
    // reserved rows, so "no rows" alone cannot distinguish "never reserved"
    // from "reserved, then cancelled" — a reserve-before-fail-closed
    // regression would still leave the table empty. The store outcome can:
    // it reports that no reservation id was ever minted.
    let outcome = env
        .state
        .store
        .consult_and_reserve_scoped_rule(&context, Timestamp::now())
        .unwrap();
    assert!(outcome.ambiguous, "two matches classify as ambiguous");
    assert!(!outcome.matched && !outcome.allow);
    assert!(
        outcome.reservation_id.is_none(),
        "selection fails closed before any budget moves; no reservation is minted"
    );
    assert!(outcome.rule.is_none(), "no rule is selected to charge");
}

/// Activation refuses a scope binding that omits a dimension the action's
/// descriptor requires (standing-rules spec: "a rule missing any required
/// dimension MUST be rejected **before activation**"). Without this the
/// incomplete rule would sit in the store looking active while being silently
/// unmatchable, because only the scope-key pre-filter would reject it.
#[tokio::test]
async fn activation_refuses_a_binding_that_omits_a_required_dimension() {
    let env = draft_env(&["thread-1"]).await;
    let grant = mint_draft_grant(&env.state, "thread-1");
    let context = resolved_context(&env.state, &grant).await;
    let complete = scoped_manifest("rule-incomplete", &context);

    // Drop one required dimension from the persisted binding.
    let mut surgery: serde_json::Value = serde_json::to_value(&complete).unwrap();
    let dimensions = surgery["reviewed_scope"]["scope"]["dimensions"]
        .as_object_mut()
        .expect("dimensions serialize as an object");
    assert!(
        dimensions.remove("counterparty").is_some(),
        "counterparty is a required dimension for email.create_draft"
    );
    let incomplete: openspine_schemas::standing_rule::StandingRuleManifest =
        serde_json::from_value(surgery).unwrap();

    let refused = env
        .state
        .store
        .activate_standing_rule(&incomplete, None, Timestamp::now())
        .expect_err("an incomplete binding must not activate");

    assert!(
        format!("{refused}").contains("Counterparty"),
        "the refusal names the omitted dimension: {refused}"
    );
    assert!(
        env.state
            .store
            .active_standing_rule_for_action(
                &openspine_schemas::action::ActionId::new("email.create_draft"),
                Timestamp::now()
            )
            .unwrap()
            .is_none(),
        "a refused activation leaves no active rule row"
    );
    assert_eq!(
        audit_count(&env.state, "standing_rule.scope_binding_rejected"),
        1,
        "the refusal leaves durable owner-actionable evidence"
    );
}

/// #128: authority for a scope-bound action is the reviewed scope, never the
/// bare action key. A legacy action-keyed rule carrying no reviewed scope is
/// not eligible for scoped admission, so it cannot admit `email.create_draft`
/// and cannot spend budget on it.
#[tokio::test]
async fn unbounded_legacy_rule_cannot_admit_scope_bound_action() {
    let env = draft_env(&["thread-1"]).await;
    mount_drafts(&env.api_server, 200, json!({"id": "draft-1"})).await;
    let grant = mint_draft_grant(&env.state, "thread-1");
    let unbounded = manifest(
        "rule-unbounded",
        "email.create_draft",
        7 * 24 * 3600,
        BudgetWindow {
            max: 5,
            window_secs: 7 * 24 * 3600,
        },
        BudgetWindow {
            max: 3,
            window_secs: 3600,
        },
        None,
    );
    assert!(unbounded.reviewed_scope.is_none());
    // Activation now refuses this outright, so the only way such a row can
    // exist is a database written before #128. Insert it directly to model
    // exactly that legacy row and prove the matcher still refuses it.
    assert!(
        env.state
            .store
            .activate_standing_rule(&unbounded, None, Timestamp::now())
            .is_err(),
        "activation refuses an unbounded rule for a scope-bound action"
    );
    insert_legacy_unbounded_rule(&env.state, &unbounded);

    let (decision, budget) = dispatch(&env.state, &grant).await;

    assert!(
        matches!(decision, GateDecision::ApprovalRequired { .. }),
        "an unbounded rule is not a reviewed scope and admits nothing here"
    );
    assert!(budget.is_none());
    assert_eq!(drafts_written(&env.api_server).await, 0);
    assert_eq!(usage_count(&env.state, "rule-unbounded", "reserved"), 0);
    assert_eq!(usage_count(&env.state, "rule-unbounded", "committed"), 0);
}

/// Containment (AD-036/D-146): the shell supplies an *intent* and nothing
/// else. A payload that tries to hand the kernel its own target, account
/// identity, counterparty, connector instance, or workflow changes no bound
/// dimension — every one is re-resolved kernel-side, so the sealed scope key
/// is byte-identical to the honest request's. A shell-chosen scope would be
/// self-granted authority.
#[tokio::test]
async fn shell_payload_cannot_supply_or_widen_a_reviewed_scope_dimension() {
    let env = draft_env(&["thread-1", "thread-2"]).await;
    let grant = mint_draft_grant(&env.state, "thread-1");
    let honest = resolved_context(&env.state, &grant).await;

    let forged = json!({
        "subject": "Re: invoice",
        "body": "Thanks - attached.",
        "target_ref": {"kind": "email_thread", "id": "thread-2"},
        "target_digest": format!("sha256:{}", "f".repeat(64)),
        "account_identity_digest": format!("sha256:{}", "e".repeat(64)),
        "account_role": "shared_workspace_mailbox",
        "connector_instance_id": "gmail_attacker_instance",
        "counterparty_identity_id": Ulid::from(99_u128).to_string(),
        "workflow_id": "attacker_workflow",
        "task_shape_digest": format!("sha256:{}", "a".repeat(64)),
    });
    let forged_ref = env
        .state
        .artifacts
        .put(canonical_json(&forged).as_bytes())
        .unwrap();
    let admission = crate::api::scoped_admission::resolve_scoped_admission(
        &env.state,
        &grant,
        &openspine_schemas::action::ActionId::new("email.create_draft"),
        Some(&forged_ref),
        Timestamp::now(),
    )
    .await
    .expect("resolution must not error");
    let crate::api::scoped_admission::ScopedAdmission::Resolved(resolved) = admission else {
        panic!("a complete grant resolves even when the payload is hostile");
    };

    assert_eq!(
        resolved.context.reviewed_scope_digest(),
        honest.reviewed_scope_digest(),
        "no shell-supplied field enters the sealed reviewed scope"
    );
    assert_eq!(
        resolved
            .context
            .target_refs()
            .first()
            .and_then(|target| target.id.as_deref()),
        Some("thread-1"),
        "the target comes from the kernel-authored selection token, not the payload"
    );
    assert_eq!(
        resolved.context.counterparty_identity_id(),
        Some(Ulid::from(11_u128)),
        "the counterparty comes from the briefcase task shape, not the payload"
    );
    assert_eq!(
        resolved
            .context
            .bound_parameters()
            .get("thread_participants")
            .map(String::as_str),
        Some("alice@example.com"),
        "the only bound parameter is the kernel-resolved participant set"
    );
    assert_eq!(
        resolved.context.bound_parameters().len(),
        1,
        "no shell payload key becomes a bound parameter"
    );
    assert_eq!(
        resolved.context.workflow_id(),
        Some("selected_thread_email_reply_draft"),
        "the workflow comes from the task grant, not the payload"
    );
}

/// Authority boundary (D-007/AD-010): a scoped rule is a composition *input*,
/// never a live authority object. It can only narrow when owner approval is
/// required — it can never turn a `gate()` denial into an effect. Selection
/// runs strictly downstream of `gate()`, so a denied action never reaches it.
#[tokio::test]
async fn scoped_rule_cannot_override_a_gate_denial() {
    let env = draft_env(&["thread-1"]).await;
    mount_drafts(&env.api_server, 200, json!({"id": "draft-1"})).await;
    let mut grant = mint_draft_grant(&env.state, "thread-1");
    let context = resolved_context(&env.state, &grant).await;
    env.state
        .store
        .activate_standing_rule(
            &scoped_manifest("rule-denied", &context),
            None,
            Timestamp::now(),
        )
        .unwrap();
    // The owner's grant explicitly denies the action the rule was reviewed for.
    grant.approval_required_actions.clear();
    grant.denied_actions = vec![openspine_schemas::action::ActionId::new(
        "email.create_draft",
    )];
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");

    let (decision, budget) = dispatch(&env.state, &grant).await;

    assert!(
        matches!(decision, GateDecision::Deny { .. }),
        "a matching reviewed scope never widens what the task grant permits"
    );
    assert!(budget.is_none());
    assert_eq!(drafts_written(&env.api_server).await, 0);
    assert_eq!(usage_count(&env.state, "rule-denied", "reserved"), 0);
    assert_eq!(usage_count(&env.state, "rule-denied", "committed"), 0);
}
