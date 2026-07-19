use super::*;
use crate::api::dispatch_tests::mint_grant_with_selection_token;
use crate::test_support::fixtures::{test_state, test_state_with_telegram};
use openspine_schemas::briefcase::{BriefcaseSection, SectionKind, VisibilityClass};
use openspine_schemas::digest::Digest;
use openspine_schemas::disclosure_policy::ClassifiedBriefcaseItem;
use openspine_schemas::standing_rule::StandingRuleManifest;
use serde_json::json;

fn provenance(class: DisclosureClass) -> DisclosureProvenance {
    DisclosureProvenance {
        items: vec![ClassifiedBriefcaseItem {
            item_ref: openspine_schemas::artifact::ArtifactRef {
                digest: Digest::parse(
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                )
                .unwrap(),
                schema_version: 1,
            },
            disclosure_class: class,
        }],
    }
}

fn request(class: DisclosureClass, relationship: RelationshipKind) -> DisclosureRequest {
    DisclosureRequest {
        raw_query: "research condition X".to_string(),
        sensitive_terms: BTreeSet::from(["condition X".to_string()]),
        action_id: ActionId::new("web.search"),
        relationship,
        provenance: provenance(class),
    }
}

#[test]
fn owner_answer_persists_as_standing_rule_with_carve_outs() {
    let state = test_state();
    let now = Timestamp::now();
    let policy = record_owner_answer(
        &state.store,
        DisclosurePolicyKey {
            relationship: RelationshipKind::Client,
            disclosure_class: DisclosureClass::Private,
        },
        EgressClass::Search,
        vec![DisclosureCarveOut {
            egress_class: EgressClass::Search,
            query_shape: openspine_schemas::digest::digest_of_bytes(b"research [redacted]"),
        }],
        now,
    )
    .unwrap();
    assert!(state
        .store
        .consult_and_reserve_standing_rule(
            &action_for_scope(
                DisclosurePolicyKey {
                    relationship: RelationshipKind::Client,
                    disclosure_class: DisclosureClass::Private
                },
                EgressClass::Search
            ),
            now
        )
        .unwrap()
        .is_some());
    assert_eq!(policy.carve_outs.len(), 1);
}

#[test]
fn disclosure_policy_recovers_carve_outs_from_store() {
    let state = test_state();
    record_owner_answer(
        &state.store,
        DisclosurePolicyKey {
            relationship: RelationshipKind::Client,
            disclosure_class: DisclosureClass::Private,
        },
        EgressClass::Search,
        vec![DisclosureCarveOut {
            egress_class: EgressClass::Search,
            query_shape: openspine_schemas::digest::digest_of_bytes(b"research [redacted]"),
        }],
        Timestamp::now(),
    )
    .unwrap();
    let loaded = state.store.load_disclosure_policies().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].carve_outs.len(), 1);
}

#[test]
fn uncovered_egress_blocks_and_produces_owner_question() {
    let state = test_state();
    let decision = check_egress(
        RelationshipKind::Client,
        OutboundQuery::from_private_context(
            "research condition X",
            &BTreeSet::from(["condition X".to_string()]),
            EgressClass::Search,
            provenance(DisclosureClass::Sensitive),
        ),
        &state.store.load_disclosure_policies().unwrap(),
    );
    let openspine_schemas::disclosure_policy::DisclosureGateDecision::Block { escalation } =
        decision
    else {
        panic!("uncovered disclosure egress must block");
    };
    let event = EscalationEvent::owner_question(
        ulid::Ulid::new(),
        escalation.question,
        None,
        Timestamp::now(),
    );
    assert!(matches!(
        event.payload,
        openspine_schemas::escalation::EscalationPayload::OwnerQuestion { .. }
    ));
}

#[tokio::test]
async fn enforcement_allows_after_owner_answer() {
    let state = test_state_with_telegram(crate::telegram::TelegramConnector::new(
        "bottest-token".to_string(),
    ));
    let (grant, _) = mint_grant_with_selection_token(
        &state,
        &["web.search"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );
    record_owner_answer(
        &state.store,
        DisclosurePolicyKey {
            relationship: RelationshipKind::Client,
            disclosure_class: DisclosureClass::Private,
        },
        EgressClass::Search,
        vec![],
        Timestamp::now(),
    )
    .unwrap();
    assert!(enforce_disclosure_egress(
        &state,
        &grant,
        request(DisclosureClass::Private, RelationshipKind::Client)
    )
    .await
    .is_ok());
}

#[test]
fn two_scopes_same_egress_neither_revoked() {
    let state = test_state();
    let now = Timestamp::now();
    record_owner_answer(
        &state.store,
        DisclosurePolicyKey {
            relationship: RelationshipKind::Client,
            disclosure_class: DisclosureClass::Private,
        },
        EgressClass::Search,
        vec![],
        now,
    )
    .unwrap();
    record_owner_answer(
        &state.store,
        DisclosurePolicyKey {
            relationship: RelationshipKind::Spouse,
            disclosure_class: DisclosureClass::Sensitive,
        },
        EgressClass::Search,
        vec![],
        now,
    )
    .unwrap();
    assert_eq!(state.store.load_disclosure_policies().unwrap().len(), 2);
    assert!(state
        .store
        .consult_and_reserve_standing_rule(
            &action_for_scope(
                DisclosurePolicyKey {
                    relationship: RelationshipKind::Client,
                    disclosure_class: DisclosureClass::Private
                },
                EgressClass::Search
            ),
            now
        )
        .unwrap()
        .is_some());
}

#[test]
fn same_scope_two_egress_classes_merge_without_erasing() {
    let state = test_state();
    let key = DisclosurePolicyKey {
        relationship: RelationshipKind::Client,
        disclosure_class: DisclosureClass::Private,
    };
    record_owner_answer(
        &state.store,
        key,
        EgressClass::Search,
        vec![],
        Timestamp::now(),
    )
    .unwrap();
    record_owner_answer(
        &state.store,
        key,
        EgressClass::ForumBrowse,
        vec![],
        Timestamp::now(),
    )
    .unwrap();
    let loaded = state.store.load_disclosure_policies().unwrap();
    assert_eq!(loaded.len(), 1);
    assert!(loaded[0]
        .allowed_egress_classes
        .contains(&EgressClass::Search));
    assert!(loaded[0]
        .allowed_egress_classes
        .contains(&EgressClass::ForumBrowse));
}

#[test]
fn disclosure_policy_does_not_revoke_existing_egress_standing_rule() {
    let state = test_state();
    let now = Timestamp::now();
    let real_rule = StandingRuleManifest {
        id: "real-search-rule".to_string(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        action_id: ActionId::new("web.search"),
        description: "owner allows web search".to_string(),
        quota: BudgetWindow {
            max: 10,
            window_secs: 3600,
        },
        rate: BudgetWindow {
            max: 2,
            window_secs: 60,
        },
        expires_after_secs: 7_776_000,
        dark_window: None,
    };
    state
        .store
        .activate_standing_rule(&real_rule, None, now)
        .unwrap();
    record_owner_answer(
        &state.store,
        DisclosurePolicyKey {
            relationship: RelationshipKind::Client,
            disclosure_class: DisclosureClass::Private,
        },
        EgressClass::Search,
        vec![],
        now,
    )
    .unwrap();
    assert!(state
        .store
        .consult_and_reserve_standing_rule(&ActionId::new("web.search"), now)
        .unwrap()
        .is_some());
}
fn forum_request(class: DisclosureClass, relationship: RelationshipKind) -> DisclosureRequest {
    DisclosureRequest {
        raw_query: "browse condition X forum".to_string(),
        sensitive_terms: BTreeSet::from(["condition X".to_string()]),
        action_id: ActionId::new("web.forum_browse"),
        relationship,
        provenance: provenance(class),
    }
}

#[tokio::test]
async fn per_egress_revocation_is_independent() {
    let state = test_state_with_telegram(crate::telegram::TelegramConnector::new(
        "bottest-token".to_string(),
    ));
    let key = DisclosurePolicyKey {
        relationship: RelationshipKind::Client,
        disclosure_class: DisclosureClass::Private,
    };
    record_owner_answer(
        &state.store,
        key,
        EgressClass::Search,
        vec![],
        Timestamp::now(),
    )
    .unwrap();
    record_owner_answer(
        &state.store,
        key,
        EgressClass::ForumBrowse,
        vec![],
        Timestamp::now(),
    )
    .unwrap();
    // Revoke only the Search envelope; ForumBrowse keeps its own.
    assert!(state
        .store
        .revoke_standing_rule("disclosure:client:private:search", Timestamp::now())
        .unwrap());
    let (search_grant, _) = mint_grant_with_selection_token(
        &state,
        &["web.search"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );
    let (forum_grant, _) = mint_grant_with_selection_token(
        &state,
        &["web.forum_browse"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );
    assert!(enforce_disclosure_egress(
        &state,
        &forum_grant,
        forum_request(DisclosureClass::Private, RelationshipKind::Client)
    )
    .await
    .is_ok());
    assert!(enforce_disclosure_egress(
        &state,
        &search_grant,
        request(DisclosureClass::Private, RelationshipKind::Client)
    )
    .await
    .is_err());
}

#[tokio::test]
async fn reanswer_after_revoke_reactivates_via_version_bump() {
    let state = test_state_with_telegram(crate::telegram::TelegramConnector::new(
        "bottest-token".to_string(),
    ));
    let key = DisclosurePolicyKey {
        relationship: RelationshipKind::Client,
        disclosure_class: DisclosureClass::Private,
    };
    record_owner_answer(
        &state.store,
        key,
        EgressClass::Search,
        vec![],
        Timestamp::now(),
    )
    .unwrap();
    let (grant, _) = mint_grant_with_selection_token(
        &state,
        &["web.search"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );
    assert!(enforce_disclosure_egress(
        &state,
        &grant,
        request(DisclosureClass::Private, RelationshipKind::Client)
    )
    .await
    .is_ok());
    assert!(state
        .store
        .revoke_standing_rule("disclosure:client:private:search", Timestamp::now())
        .unwrap());
    assert!(enforce_disclosure_egress(
        &state,
        &grant,
        request(DisclosureClass::Private, RelationshipKind::Client)
    )
    .await
    .is_err());
    // Re-answer: the version must bump so reactivation is not a no-op.
    record_owner_answer(
        &state.store,
        key,
        EgressClass::Search,
        vec![],
        Timestamp::now(),
    )
    .unwrap();
    assert_eq!(
        state
            .store
            .standing_rule_version_for_action(&action_for_scope(key, EgressClass::Search))
            .unwrap(),
        Some(2)
    );
    assert!(enforce_disclosure_egress(
        &state,
        &grant,
        request(DisclosureClass::Private, RelationshipKind::Client)
    )
    .await
    .is_ok());
}
#[tokio::test]
async fn prepared_query_mints_consumes_once_and_verifies_digest() {
    let state = test_state_with_telegram(crate::telegram::TelegramConnector::new(
        "bottest-token".to_string(),
    ));
    let sections = vec![BriefcaseSection {
        key: "private-query-term".to_string(),
        kind: SectionKind::Preference,
        visibility: VisibilityClass::WorkerScratch,
        depth: 0,
        disclosure_class: Some(DisclosureClass::Private),
        payload: json!("condition X"),
    }];
    let grant_id = ulid::Ulid::new();
    let ref_ = prepare_disclosure_query(
        &state,
        grant_id,
        ActionId::new("web.search"),
        "research condition X".to_string(),
        RelationshipKind::Client,
        EgressClass::Search,
        &sections,
    )
    .await
    .unwrap();
    let prepared = state
        .store
        .consume_prepared_query(&ref_)
        .unwrap()
        .expect("prepared query present");
    assert_eq!(prepared.generalized_query, "research [redacted]");
    assert_eq!(prepared.digest, ref_.digest);
    // Second consume fails: the token is one-use.
    assert!(state.store.consume_prepared_query(&ref_).unwrap().is_none());
    // A tampered digest reference is rejected.
    let tampered = PreparedQueryRef {
        id: ref_.id.clone(),
        digest: openspine_schemas::digest::Digest::parse(
            "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap(),
    };
    assert!(state
        .store
        .consume_prepared_query(&tampered)
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn owner_disclosure_answer_creates_policy_and_clears_pending() {
    let state = test_state_with_telegram(crate::telegram::TelegramConnector::new(
        "bottest-token".to_string(),
    ));
    let key = DisclosurePolicyKey {
        relationship: RelationshipKind::Client,
        disclosure_class: DisclosureClass::Private,
    };
    let pending_id = ulid::Ulid::new();
    state
        .store
        .store_disclosure_pending_question(
            &pending_id,
            ulid::Ulid::new(),
            key.relationship,
            key.disclosure_class,
            EgressClass::Search,
            openspine_schemas::digest::digest_of_bytes(b"research [redacted]"),
            Timestamp::now(),
        )
        .unwrap();
    crate::pipeline::handle_owner_update(
        &state,
        &crate::test_support::fixtures::owner_update(&format!("/disclosure allow {pending_id}")),
    )
    .await
    .unwrap();
    let policies = state.store.load_disclosure_policies().unwrap();
    assert_eq!(policies.len(), 1);
    assert!(policies[0]
        .allowed_egress_classes
        .contains(&EgressClass::Search));
    assert!(state
        .store
        .load_disclosure_pending_question(&pending_id)
        .unwrap()
        .is_none());
}
