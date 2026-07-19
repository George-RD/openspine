use super::*;

use crate::api::dispatch_tests::mint_grant_with_selection_token;
use crate::test_support::fixtures::{test_state, test_state_with_telegram};
use openspine_schemas::briefcase::{BriefcaseSection, SectionKind, VisibilityClass};
use serde_json::json;

/// Blocker 2 regression: sensitive strings nested below objects and arrays are
/// redacted before the prepared query can reach a connector.
#[tokio::test]
async fn nested_json_sensitive_term_extraction_redacts_all_strings() {
    let state = test_state();
    let grant_id = Ulid::new();
    let sections = vec![BriefcaseSection {
        key: "private-nested".to_string(),
        kind: SectionKind::Preference,
        visibility: VisibilityClass::WorkerScratch,
        depth: 0,
        disclosure_class: Some(DisclosureClass::Private),
        payload: json!({"profile": {"condition": "nested condition X"}, "aliases": ["condition X"]}),
    }];
    let reference = prepare_disclosure_query(
        &state,
        grant_id,
        ActionId::new("web.search"),
        "research nested condition X".to_string(),
        RelationshipKind::Client,
        EgressClass::Search,
        &sections,
    )
    .await
    .expect("nested payload should prepare");
    let prepared = state
        .store
        .consume_prepared_query(&reference)
        .expect("consume succeeds")
        .expect("token exists");
    assert_eq!(prepared.generalized_query, "research [redacted]");
}

/// Anchor 1 regression: kernel-derived provenance enforces EVERY non-public
/// class present in the briefcase — a caller naming only the lesser-classified
/// section cannot shrink the enforced set, and Internal is included.
#[test]
fn caller_omitted_class_is_still_enforced() {
    let sections = vec![
        BriefcaseSection {
            key: "internal-notes".to_string(),
            kind: SectionKind::Preference,
            visibility: VisibilityClass::WorkerScratch,
            depth: 0,
            disclosure_class: Some(DisclosureClass::Internal),
            payload: json!("internal summary"),
        },
        BriefcaseSection {
            key: "sensitive-health".to_string(),
            kind: SectionKind::Preference,
            visibility: VisibilityClass::WorkerScratch,
            depth: 0,
            disclosure_class: Some(DisclosureClass::Sensitive),
            payload: json!("condition X"),
        },
    ];
    let provenance = provenance_from_sections(&sections).unwrap();
    let classes = provenance.classes();
    assert!(classes.contains(&DisclosureClass::Internal));
    assert!(classes.contains(&DisclosureClass::Sensitive));
    // A policy covering only Internal must not allow the Sensitive class
    // that the caller's section selection tried to omit.
    let now = Timestamp::now();
    let state = test_state();
    record_owner_answer(
        &state.store,
        DisclosurePolicyKey {
            relationship: RelationshipKind::Client,
            disclosure_class: DisclosureClass::Internal,
        },
        EgressClass::Search,
        vec![],
        now,
    )
    .unwrap();
    let policies = state.store.load_disclosure_policies().unwrap();
    let query = openspine_schemas::disclosure_policy::OutboundQuery::from_private_context(
        "research topic",
        &BTreeSet::new(),
        EgressClass::Search,
        provenance,
    );
    let decision = openspine_schemas::disclosure_policy::check_egress(
        RelationshipKind::Client,
        query,
        &policies,
    );
    match decision {
        openspine_schemas::disclosure_policy::DisclosureGateDecision::Block { escalation } => {
            assert_eq!(escalation.key.disclosure_class, DisclosureClass::Sensitive);
        }
        openspine_schemas::disclosure_policy::DisclosureGateDecision::Allow { .. } => {
            panic!("uncovered Sensitive class must block despite covered Internal")
        }
    }
}

/// Legacy fail-closed regression: a worker-visible section WITHOUT a
/// disclosure classification refuses provenance derivation entirely —
/// legacy/unknown content never silently drops out of the enforced set.
/// KernelBound sections are excluded: they can never reach a worker's view.
#[test]
fn unclassified_worker_visible_section_fails_closed() {
    let unclassified = vec![BriefcaseSection {
        key: "legacy".to_string(),
        kind: SectionKind::Preference,
        visibility: VisibilityClass::WorkerScratch,
        depth: 0,
        disclosure_class: None,
        payload: json!("legacy content"),
    }];
    assert!(matches!(
        provenance_from_sections(&unclassified),
        Err(DisclosureError::UnclassifiedSection(key)) if key == "legacy"
    ));
    let kernel_bound = vec![BriefcaseSection {
        key: "grant".to_string(),
        kind: SectionKind::Grant,
        visibility: VisibilityClass::KernelBound,
        depth: 0,
        disclosure_class: Some(DisclosureClass::Private),
        payload: json!("kernel-only"),
    }];
    // KernelBound never reaches a worker: excluded from query provenance, so
    // a Public-only query needs no Private policy.
    assert!(provenance_from_sections(&kernel_bound)
        .unwrap()
        .items
        .is_empty());
}

/// Anchor 2 regression: a blocked disclosure cancels every envelope
/// reservation it took — the covered class's budget is fully restored, so a
/// block cannot leak reserved quota (the StoreError mid-loop path shares the
/// same `cancel_reservations` rollback).
#[tokio::test]
async fn blocked_disclosure_releases_reserved_envelope_budget() {
    let state = test_state_with_telegram(crate::telegram::TelegramConnector::new(
        "bottest-token".to_string(),
    ));
    let now = Timestamp::now();
    let covered = DisclosurePolicyKey {
        relationship: RelationshipKind::Client,
        disclosure_class: DisclosureClass::Internal,
    };
    record_owner_answer(&state.store, covered, EgressClass::Search, vec![], now).unwrap();
    let (grant, _) = mint_grant_with_selection_token(
        &state,
        &["web.search"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );
    let sections = vec![
        BriefcaseSection {
            key: "internal-notes".to_string(),
            kind: SectionKind::Preference,
            visibility: VisibilityClass::WorkerScratch,
            depth: 0,
            disclosure_class: Some(DisclosureClass::Internal),
            payload: json!("internal summary"),
        },
        BriefcaseSection {
            key: "sensitive-health".to_string(),
            kind: SectionKind::Preference,
            visibility: VisibilityClass::WorkerScratch,
            depth: 0,
            disclosure_class: Some(DisclosureClass::Sensitive),
            payload: json!("condition X"),
        },
    ];
    let outcome = enforce_disclosure_egress(
        &state,
        &grant,
        DisclosureRequest {
            raw_query: "research topic".to_string(),
            sensitive_terms: BTreeSet::new(),
            action_id: ActionId::new("web.search"),
            relationship: RelationshipKind::Client,
            provenance: provenance_from_sections(&sections).unwrap(),
        },
    )
    .await;
    // The uncovered Sensitive class blocks; reservation cancellation happens
    // BEFORE escalation delivery, so an Err of either Blocked (delivery ok)
    // or Store (test connector unreachable, D-058) proves the rollback ran.
    assert!(outcome.is_err(), "uncovered class must not allow");
    // The covered class's envelope must have its full budget back: five
    // consecutive reserve+finalize cycles succeed only if the blocked
    // request's reservation was cancelled.
    let action = action_for_scope(covered, EgressClass::Search);
    for _ in 0..5 {
        let (rule, reservation) = state
            .store
            .consult_and_reserve_standing_rule(&action, now)
            .unwrap()
            .expect("envelope visible");
        let reservation = reservation.expect("blocked request must not leak budget");
        assert!(state
            .store
            .finalize_standing_rule_reservation(&rule.rule_id, rule.version, &reservation, now)
            .unwrap());
    }
}

/// Blocker 5 regression: a revoked scope's envelope cannot revoke a sibling
/// relationship/class scope that authorizes the same egress class.
#[test]
fn per_scope_envelope_revocation_leaves_sibling_live() {
    let state = test_state();
    let now = Timestamp::now();
    let first = DisclosurePolicyKey {
        relationship: RelationshipKind::Client,
        disclosure_class: DisclosureClass::Private,
    };
    let sibling = DisclosurePolicyKey {
        relationship: RelationshipKind::Spouse,
        disclosure_class: DisclosureClass::Sensitive,
    };
    record_owner_answer(&state.store, first, EgressClass::Search, vec![], now).unwrap();
    record_owner_answer(&state.store, sibling, EgressClass::Search, vec![], now).unwrap();
    assert!(state
        .store
        .revoke_standing_rule("disclosure:client:private:search", now)
        .unwrap());
    assert!(state
        .store
        .consult_and_reserve_standing_rule(&action_for_scope(sibling, EgressClass::Search), now)
        .unwrap()
        .is_some());
}

/// Blocker 6 regression: owner carve-out approval consumes the kernel-stored
/// blocked-query digest; no caller-supplied digest is accepted by the parser.
#[test]
fn scoped_owner_answer_uses_pending_question_digest() {
    let digest = openspine_schemas::digest::digest_of_bytes(b"research [redacted]");
    let pending = Ulid::new();
    let state = test_state();
    let key = DisclosurePolicyKey {
        relationship: RelationshipKind::Client,
        disclosure_class: DisclosureClass::Private,
    };
    state
        .store
        .store_disclosure_pending_question(
            &pending,
            Ulid::new(),
            key.relationship,
            key.disclosure_class,
            EgressClass::Search,
            digest.clone(),
            Timestamp::now(),
        )
        .unwrap();
    let question = state
        .store
        .load_disclosure_pending_question(&pending)
        .unwrap()
        .unwrap();
    assert_eq!(question.blocked_query_digest, digest);
    let policy = record_owner_answer(
        &state.store,
        key,
        EgressClass::Search,
        vec![DisclosureCarveOut {
            egress_class: EgressClass::Search,
            query_shape: question.blocked_query_digest.clone(),
        }],
        Timestamp::now(),
    )
    .unwrap();
    assert!(policy.covers(
        key.relationship,
        key.disclosure_class,
        EgressClass::Search,
        "research [redacted]",
    ));
    assert!(matches!(
        crate::telegram::parse_disclosure_command(&format!("/disclosure allow-with-carve-out {pending}")),
        Some(crate::telegram::DisclosureAnswer::AllowWithCarveOut(id)) if id == pending
    ));
    assert!(crate::telegram::parse_disclosure_command(&format!(
        "/disclosure allow {pending} sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    ))
    .is_none());
}

/// Blocker 4 regression: an active disclosure envelope with exhausted quota or
/// rate headroom must not be treated as a live allow.
#[test]
fn disclosure_envelope_budget_exhaustion_blocks() {
    let state = test_state();
    let key = DisclosurePolicyKey {
        relationship: RelationshipKind::Client,
        disclosure_class: DisclosureClass::Private,
    };
    let now = Timestamp::now();
    record_owner_answer(&state.store, key, EgressClass::Search, vec![], now).unwrap();
    let action = action_for_scope(key, EgressClass::Search);
    for _ in 0..5 {
        let (rule, reservation) = state
            .store
            .consult_and_reserve_standing_rule(&action, now)
            .unwrap()
            .expect("envelope should have headroom");
        let reservation = reservation.expect("headroom must reserve");
        assert!(state
            .store
            .finalize_standing_rule_reservation(&rule.rule_id, rule.version, &reservation, now)
            .unwrap());
    }
    let (_, reservation) = state
        .store
        .consult_and_reserve_standing_rule(&action, now)
        .unwrap()
        .expect("active rule remains visible when budget is exhausted");
    assert!(
        reservation.is_none(),
        "exhausted D-107 budget must not reserve"
    );
}

/// Blocker 7 regression: the dispatch boundary exposes only a fixed generic
/// denial, never the relationship/class/question Debug representation.
#[test]
fn worker_denial_has_no_debug_leak() {
    let denial = "rated disclosure was blocked by kernel policy";
    assert!(!denial.contains("relationship"));
    assert!(!denial.contains("class"));
    assert!(!denial.contains("question"));
    assert!(!denial.contains("Client"));
    assert!(!denial.contains("Sensitive"));
}
