//! Store-level tests for scope-matched standing-rule admission (#128).
//!
//! These drive `consult_and_reserve_scoped_rule` directly against the
//! in-memory store with a `ResolvedActionContext` built for `email.create_draft`
//! (the only action with a registered implementation descriptor). They assert
//! the 0/1/2+ classification, the disjoint-scope coexistence, the no-budget
//! guarantees for mismatch and ambiguity, the corrupt-binding fail-closed
//! case, and the atomic selection-before-reservation ordering.

use std::collections::BTreeMap;

use jiff::Timestamp;
use openspine_schemas::action::{ActionId, ActionImplementationId};
use openspine_schemas::briefcase::CounterpartyRef;
use openspine_schemas::digest::Digest;
use openspine_schemas::event::{AccountRole, TargetRef, TargetRefKind};
use openspine_schemas::identity::RelationshipKind;
use openspine_schemas::resolved_context::{ResolvedActionContext, ResolvedActionContextInput};
use openspine_schemas::reviewed_scope::ReviewedActionScope;
use openspine_schemas::standing_rule::{BudgetWindow, StandingRuleManifest};
use ulid::Ulid;

use super::standing_rules_tests::manifest;
use super::Store;

fn digest(c: char) -> Digest {
    Digest::parse(format!("sha256:{}", c.to_string().repeat(64))).unwrap()
}

/// A `ResolvedActionContext` for `email.create_draft` resolved from the
/// canonical catalog, bound to a concrete instance (connector, account,
/// target, counterparty, workflow, task shape).
fn email_context() -> ResolvedActionContext {
    let catalog = crate::action_catalog::canonical_catalog();
    let action = ActionId::new("email.create_draft");
    let implementation = ActionImplementationId::new("gmail.draft.v1");
    let input = ResolvedActionContextInput {
        connector_instance_id: "gmail-primary".into(),
        account_role: Some(AccountRole::OwnerMailbox),
        account_identity_digest: Some(digest('a')),
        target_refs: vec![TargetRef {
            kind: TargetRefKind::EmailThread,
            id: Some("thread-1".into()),
        }],
        counterparty: Some(CounterpartyRef::Bound {
            identity_id: Ulid::from(11_u128),
            relationship: RelationshipKind::Client,
        }),
        bound_parameters: BTreeMap::new(),
        target_digest: Some(digest('b')),
        payload_digest: Some(digest('c')),
        workflow_id: Some("draft_reply_workflow".into()),
        task_shape_digest: Some(digest('d')),
    };
    ResolvedActionContext::try_new(&catalog, &action, &implementation, input).unwrap()
}

/// A scoped `StandingRuleManifest` for `email.create_draft` bound to a
/// specific context via its reviewed scope and compatibility epoch.
fn scoped_manifest(id: &str, context: &ResolvedActionContext) -> StandingRuleManifest {
    let scope = ReviewedActionScope::derive(context).unwrap();
    let binding = openspine_schemas::standing_rule::ReviewedScopeBinding::derive_from(
        scope,
        context.compatibility_digest().clone(),
    );
    let mut m = manifest(
        id,
        "email.create_draft",
        7 * 24 * 3600,
        BudgetWindow {
            max: 5,
            window_secs: 7 * 24 * 3600,
        },
        BudgetWindow {
            max: 1,
            window_secs: 3600,
        },
        None,
    );
    m.reviewed_scope = Some(binding);
    m
}

fn reserved_usage_count(store: &Store, rule_id: &str) -> i64 {
    store
        .conn
        .lock()
        .query_row(
            "SELECT COUNT(DISTINCT reservation_id) FROM standing_rule_usage \
             WHERE rule_id = ?1 AND status = 'reserved'",
            rusqlite::params![rule_id],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn scoped_consult_admits_exactly_one_matching_rule_and_reserves_budget() {
    let store = Store::open_in_memory().unwrap();
    let now = Timestamp::now();
    let context = email_context();
    store
        .activate_standing_rule(&scoped_manifest("rule-a", &context), None, now)
        .unwrap();

    let outcome = store
        .consult_and_reserve_scoped_rule(&context, now)
        .unwrap();
    assert!(outcome.matched && outcome.allow && !outcome.ambiguous);
    let rule = outcome.rule.expect("matched rule");
    assert_eq!(rule.rule_id, "rule-a");
    let reservation = outcome.reservation_id.expect("reserved");
    assert_eq!(reserved_usage_count(&store, "rule-a"), 1);
    assert!(store
        .standing_rule_is_current("rule-a", rule.version)
        .unwrap());
    assert!(reservation.starts_with("0"), "reservation is a ULID");
}

#[test]
fn scoped_consult_zero_matches_falls_back_with_no_budget() {
    let store = Store::open_in_memory().unwrap();
    let now = Timestamp::now();
    let context = email_context();
    // A rule bound to a *different* account identity does not match.
    let catalog = crate::action_catalog::canonical_catalog();
    let other = ResolvedActionContext::try_new(
        &catalog,
        &ActionId::new("email.create_draft"),
        &ActionImplementationId::new("gmail.draft.v1"),
        ResolvedActionContextInput {
            connector_instance_id: "gmail-primary".into(),
            account_role: Some(AccountRole::OwnerMailbox),
            account_identity_digest: Some(digest('e')),
            target_refs: vec![TargetRef {
                kind: TargetRefKind::EmailThread,
                id: Some("thread-1".into()),
            }],
            counterparty: Some(CounterpartyRef::Bound {
                identity_id: Ulid::from(11_u128),
                relationship: RelationshipKind::Client,
            }),
            bound_parameters: BTreeMap::new(),
            target_digest: Some(digest('b')),
            payload_digest: Some(digest('c')),
            workflow_id: Some("draft_reply_workflow".into()),
            task_shape_digest: Some(digest('d')),
        },
    )
    .unwrap();

    store
        .activate_standing_rule(&scoped_manifest("rule-other", &other), None, now)
        .unwrap();

    // `context` does not match the rule bound to `other`.
    let outcome = store
        .consult_and_reserve_scoped_rule(&context, now)
        .unwrap();
    assert!(!outcome.matched && !outcome.allow && !outcome.ambiguous);
    assert!(outcome.rule.is_none() && outcome.reservation_id.is_none());
    assert_eq!(reserved_usage_count(&store, "rule-other"), 0);
}

#[test]
fn scoped_consult_ambiguous_overlap_fails_closed_with_no_budget() {
    use jiff::Timestamp;

    let store = Store::open_in_memory().unwrap();
    let now = Timestamp::now();
    let context = email_context();

    // The activation path revokes an *identical-scope* prior rule, so a true
    // two-match state cannot normally survive it. But the ambiguity can still
    // arise in the store (a race, legacy rows, or a scope that overlaps by
    // review while its digest columns differ). Insert two rules that both
    // match the same context directly into the store to construct the
    // fail-closed case the matcher must handle, then assert it refuses with
    // no budget and durable owner-actionable evidence.
    let m_a = scoped_manifest("rule-a", &context);
    let m_b = scoped_manifest("rule-b", &context);
    store.activate_standing_rule(&m_a, None, now).unwrap();
    // Force the second row in without the coexistence revoke by inserting via
    // SQL with a distinct rule_id but the same reviewed_scope_digest.
    let binding = m_b.reviewed_scope.as_ref().unwrap();
    let scope_digest = binding.reviewed_scope_digest.as_str();
    let compat = binding.compatibility_digest.as_str();
    let rule_json = serde_json::to_string(&m_b).unwrap();
    let activated_at = Timestamp::now().as_nanosecond() as i64;
    {
        let conn = store.conn.lock();
        conn.execute(
            "INSERT INTO standing_rules (
                rule_id, artifact_id, version, action_id, rule_json,
                quota_max, quota_window_secs, rate_max, rate_window_secs,
                expires_after_secs, dark_window_timeout_secs, dark_window_default,
                status, activated_at, last_used_at, revoked_at, needs_review_since,
                reviewed_scope_digest, compatibility_digest
            ) VALUES (?1, ?1, 1, ?2, ?3, 5, 604800, 1, 3600, 604800, NULL, NULL,
                      'active', ?4, NULL, NULL, NULL, ?5, ?6)",
            rusqlite::params![
                "rule-b",
                "email.create_draft",
                rule_json,
                activated_at,
                scope_digest,
                compat,
            ],
        )
        .unwrap();
    }
    assert!(
        store
            .active_standing_rule_for_action(&ActionId::new("email.create_draft"), now)
            .unwrap()
            .is_some(),
        "scoped rules coexist"
    );

    // Both rules match the same context: admission fails closed — no
    // reservation, durable owner-actionable evidence, no timer.
    let outcome = store
        .consult_and_reserve_scoped_rule(&context, now)
        .unwrap();
    assert!(outcome.ambiguous, "overlap must be flagged");
    assert!(!outcome.matched && !outcome.allow);
    assert!(outcome.rule.is_none() && outcome.reservation_id.is_none());
    assert_eq!(reserved_usage_count(&store, "rule-a"), 0);
    assert_eq!(reserved_usage_count(&store, "rule-b"), 0);
    assert_eq!(
        store
            .count_audit_events_of_kind("standing_rule.ambiguous_scope_overlap")
            .unwrap(),
        1
    );
}

#[test]
fn scoped_consult_corrupt_binding_fails_closed_as_invalid_scope() {
    let store = Store::open_in_memory().unwrap();
    let now = Timestamp::now();
    let context = email_context();
    // Build a manifest whose stored scope-key digest disagrees with its values
    // (the corrupt-binding case). Activate it, then corrupt the row's digest.
    let mut m = scoped_manifest("rule-corrupt", &context);
    m.reviewed_scope = Some(
        openspine_schemas::standing_rule::ReviewedScopeBinding::derive_from(
            ReviewedActionScope::derive(&context).unwrap(),
            context.compatibility_digest().clone(),
        ),
    );
    store.activate_standing_rule(&m, None, now).unwrap();
    // Corrupt the persisted binding digest to disagree with the values.
    store
        .conn
        .lock()
        .execute(
            "UPDATE standing_rules SET reviewed_scope_digest = ?1 WHERE rule_id = 'rule-corrupt'",
            rusqlite::params![
                "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
            ],
        )
        .unwrap();

    let outcome = store
        .consult_and_reserve_scoped_rule(&context, now)
        .unwrap();
    assert!(
        !outcome.matched && !outcome.allow && !outcome.ambiguous,
        "corrupt binding must not match on either half"
    );
    assert!(outcome.rule.is_none() && outcome.reservation_id.is_none());
    assert_eq!(reserved_usage_count(&store, "rule-corrupt"), 0);
}

#[test]
fn scoped_consult_disjoint_scoped_rules_match_only_their_own_context() {
    let store = Store::open_in_memory().unwrap();
    let now = Timestamp::now();
    let context_a = email_context();
    // A second context differing only in the canonical target.
    let catalog = crate::action_catalog::canonical_catalog();
    let context_b = ResolvedActionContext::try_new(
        &catalog,
        &ActionId::new("email.create_draft"),
        &ActionImplementationId::new("gmail.draft.v1"),
        ResolvedActionContextInput {
            connector_instance_id: "gmail-primary".into(),
            account_role: Some(AccountRole::OwnerMailbox),
            account_identity_digest: Some(digest('a')),
            target_refs: vec![TargetRef {
                kind: TargetRefKind::EmailThread,
                id: Some("thread-2".into()),
            }],
            counterparty: Some(CounterpartyRef::Bound {
                identity_id: Ulid::from(11_u128),
                relationship: RelationshipKind::Client,
            }),
            bound_parameters: BTreeMap::new(),
            target_digest: Some(digest('b')),
            payload_digest: Some(digest('c')),
            workflow_id: Some("draft_reply_workflow".into()),
            task_shape_digest: Some(digest('d')),
        },
    )
    .unwrap();
    assert_ne!(
        context_a.reviewed_scope_digest().unwrap(),
        context_b.reviewed_scope_digest().unwrap()
    );

    store
        .activate_standing_rule(&scoped_manifest("rule-thread1", &context_a), None, now)
        .unwrap();
    store
        .activate_standing_rule(&scoped_manifest("rule-thread2", &context_b), None, now)
        .unwrap();

    // context_a admits only rule-thread1.
    let a = store
        .consult_and_reserve_scoped_rule(&context_a, now)
        .unwrap();
    assert!(a.matched && a.allow);
    assert_eq!(a.rule.unwrap().rule_id, "rule-thread1");
    assert_eq!(reserved_usage_count(&store, "rule-thread1"), 1);
    assert_eq!(reserved_usage_count(&store, "rule-thread2"), 0);

    // context_b admits only rule-thread2; each holds independent budget.
    let b = store
        .consult_and_reserve_scoped_rule(&context_b, now)
        .unwrap();
    assert!(b.matched && b.allow);
    assert_eq!(b.rule.unwrap().rule_id, "rule-thread2");
    assert_eq!(reserved_usage_count(&store, "rule-thread2"), 1);
    assert_eq!(
        reserved_usage_count(&store, "rule-thread1"),
        1,
        "no pooling"
    );
}

#[test]
fn scoped_consult_declaration_drift_stops_matching_before_budget() {
    use openspine_schemas::action::ActionCatalog;

    let store = Store::open_in_memory().unwrap();
    let now = Timestamp::now();
    let context = email_context();
    store
        .activate_standing_rule(&scoped_manifest("rule-a", &context), None, now)
        .unwrap();

    // A context differing only in a declaration axis (descriptor version)
    // changes the compatibility epoch; the rule bound to the old epoch must
    // stop matching. Rebuild a catalog whose descriptor_version is bumped,
    // reusing the same implementation descriptor and egress declaration.
    let action_id = ActionId::new("email.create_draft");
    let canonical = crate::action_catalog::canonical_catalog();
    let mut descriptor = canonical
        .delegation_descriptor_for(&action_id)
        .expect("email.create_draft delegation descriptor")
        .clone();
    descriptor.descriptor_version = descriptor.descriptor_version.saturating_add(1);
    let implementation = canonical
        .implementation_descriptor_for_action(&action_id)
        .unwrap()
        .clone();
    let implementation_id = implementation.implementation_id.clone();
    let catalog = ActionCatalog::new([action_id.clone()])
        .with_egress_declarations([(
            action_id.clone(),
            canonical.egress_decl_for(&action_id).unwrap().clone(),
        )])
        .with_delegation_descriptors([descriptor])
        .with_implementation_descriptors([implementation]);
    let drifted = ResolvedActionContext::try_new(
        &catalog,
        &action_id,
        &implementation_id,
        ResolvedActionContextInput {
            connector_instance_id: "gmail-primary".into(),
            account_role: Some(AccountRole::OwnerMailbox),
            account_identity_digest: Some(digest('a')),
            target_refs: vec![TargetRef {
                kind: TargetRefKind::EmailThread,
                id: Some("thread-1".into()),
            }],
            counterparty: Some(CounterpartyRef::Bound {
                identity_id: Ulid::from(11_u128),
                relationship: RelationshipKind::Client,
            }),
            bound_parameters: BTreeMap::new(),
            target_digest: Some(digest('b')),
            payload_digest: Some(digest('c')),
            workflow_id: Some("draft_reply_workflow".into()),
            task_shape_digest: Some(digest('d')),
        },
    )
    .unwrap();

    let outcome = store
        .consult_and_reserve_scoped_rule(&drifted, now)
        .unwrap();
    assert!(
        !outcome.matched && !outcome.allow && !outcome.ambiguous,
        "declaration drift must restore ordinary approval before budget"
    );
    assert_eq!(reserved_usage_count(&store, "rule-a"), 0);
}

fn rule_status(store: &Store, rule_id: &str) -> String {
    store
        .conn
        .lock()
        .query_row(
            "SELECT status FROM standing_rules WHERE rule_id = ?1",
            rusqlite::params![rule_id],
            |row| row.get(0),
        )
        .unwrap()
}

/// #176 regression: one unparseable `rule_json` row must not abort the whole
/// crypto-erase sweep. Poison a single live row, then assert every other
/// matching counterparty rule is still revoked, the durable erased-scope marker
/// still lands, and the malformed row is isolated (left intact) and surfaced via
/// a durable audit row instead of `?`-aborting the transaction.
#[test]
fn erasure_sweep_isolates_unparseable_rule_json_and_still_erases() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts =
        crate::artifact_store::ArtifactStore::open(dir.path().join("artifacts"), [7u8; 32])
            .unwrap();
    let now = Timestamp::now();
    let context = email_context();
    // `email_context` binds this counterparty via its reviewed scope.
    let counterparty = Ulid::from(11_u128);

    // A well-formed standing rule scoped to the counterparty under erasure.
    store
        .activate_standing_rule(&scoped_manifest("rule-good", &context), None, now)
        .unwrap();

    // Poison: a second live row whose `rule_json` cannot deserialize into a
    // `StandingRuleManifest`. Insert via SQL so activation validation is bypassed
    // and the coexistence revoke never fires.
    {
        let conn = store.conn.lock();
        conn.execute(
            "INSERT INTO standing_rules (
                rule_id, artifact_id, version, action_id, rule_json,
                quota_max, quota_window_secs, rate_max, rate_window_secs,
                expires_after_secs, dark_window_timeout_secs, dark_window_default,
                status, activated_at, last_used_at, revoked_at, needs_review_since,
                reviewed_scope_digest, compatibility_digest
            ) VALUES (?1, ?1, 1, ?2, ?3, 5, 604800, 1, 3600, 604800, NULL, NULL,
                      'active', ?4, NULL, NULL, NULL, NULL, NULL)",
            rusqlite::params![
                "rule-bad",
                "email.create_draft",
                "{ this is not valid manifest json",
                Timestamp::now().as_nanosecond() as i64,
            ],
        )
        .unwrap();
    }

    // The sweep must complete despite the poison row.
    store
        .mark_learned_artifacts_erased(counterparty, &artifacts)
        .unwrap();

    // Every other counterparty rule still erases.
    assert_eq!(
        rule_status(&store, "rule-good"),
        "revoked",
        "a poison row must not stop a matching rule from being revoked"
    );
    // The durable erased-scope marker still lands.
    assert!(
        store.is_counterparty_erased(counterparty).unwrap(),
        "erased-scope marker must be inserted despite the poison row"
    );
    // The malformed row is isolated: skipped and left intact for operator triage.
    assert_eq!(
        rule_status(&store, "rule-bad"),
        "active",
        "the unparseable row is isolated, not mutated"
    );
    // ...and surfaced durably rather than silently dropped.
    assert_eq!(
        store
            .count_audit_events_of_kind("standing_rule.malformed_row_skipped")
            .unwrap(),
        1,
        "the skipped unparseable row must leave durable audit evidence"
    );
}
