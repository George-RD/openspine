use super::*;
use crate::digest::Digest;
fn item(class: DisclosureClass) -> ClassifiedBriefcaseItem {
    item_with_origin(class, None)
}

fn item_with_origin(
    class: DisclosureClass,
    origin: Option<ProvenanceOrigin>,
) -> ClassifiedBriefcaseItem {
    ClassifiedBriefcaseItem {
        item_ref: ArtifactRef {
            digest: Digest::parse(
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            )
            .expect("digest"),
            schema_version: 1,
        },
        disclosure_class: class,
        origin,
    }
}

fn query(class: DisclosureClass) -> OutboundQuery {
    OutboundQuery::from_private_context(
        "research condition X",
        &BTreeSet::from(["condition X".to_string()]),
        EgressClass::Search,
        DisclosureProvenance {
            // System origin so the coverage-focused cases below are unaffected
            // by the origin closure (system data reaches any recipient).
            items: vec![item_with_origin(class, Some(ProvenanceOrigin::System {}))],
        },
        RecipientIdentity::Unresolved,
    )
}

fn policy(class: DisclosureClass) -> DisclosurePolicy {
    DisclosurePolicy {
        id: "policy:known:private".to_string(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        key: DisclosurePolicyKey {
            relationship: RelationshipKind::Client,
            disclosure_class: class,
        },
        allowed_egress_classes: vec![EgressClass::Search],
        standing_rule_bindings: Default::default(),
        carve_outs: vec![],
    }
}

#[test]
fn private_context_query_is_an_effect_and_generalized_before_egress() {
    let outbound = query(DisclosureClass::Private);
    assert!(outbound.is_effect());
    assert_eq!(outbound.generalized_query, "research [redacted]");
}

#[test]
fn uncovered_disclosure_class_blocks_and_produces_owner_question_escalation() {
    let decision = check_egress(
        RelationshipKind::Client,
        query(DisclosureClass::Sensitive),
        &[],
        &[],
    );
    let DisclosureGateDecision::Block { escalation } = decision else {
        panic!("uncovered sensitive egress must block");
    };
    assert_eq!(escalation.key.disclosure_class, DisclosureClass::Sensitive);
    assert_eq!(escalation.egress_class, EgressClass::Search);
    assert_eq!(
        escalation.question,
        "Can I share this kind of information with this relationship through this channel?"
    );
}

#[test]
fn coverage_uses_provenance_even_when_generalized_text_is_public() {
    let mut outbound = query(DisclosureClass::Private);
    outbound.generalized_query = "public research topic".to_string();
    assert!(matches!(
        check_egress(RelationshipKind::Client, outbound, &[], &[]),
        DisclosureGateDecision::Block { .. }
    ));
}

#[test]
fn active_relationship_and_class_policy_allows_covered_egress() {
    assert!(matches!(
        check_egress(
            RelationshipKind::Client,
            query(DisclosureClass::Private),
            &[policy(DisclosureClass::Private)],
            &[],
        ),
        DisclosureGateDecision::Allow { .. }
    ));
}

#[test]
fn public_context_does_not_require_relationship_policy() {
    assert!(matches!(
        check_egress(
            RelationshipKind::Vendor,
            query(DisclosureClass::Public),
            &[],
            &[],
        ),
        DisclosureGateDecision::Allow { .. }
    ));
}

#[test]
fn carve_out_extends_covered_egress_without_new_policy() {
    let mut covered = policy(DisclosureClass::Private);
    covered.allowed_egress_classes = vec![];
    covered.carve_outs = vec![DisclosureCarveOut {
        egress_class: EgressClass::Search,
        query_shape: digest_of_bytes(b"research [redacted]"),
    }];
    assert!(matches!(
        check_egress(
            RelationshipKind::Client,
            query(DisclosureClass::Private),
            &[covered],
            &[],
        ),
        DisclosureGateDecision::Allow { .. }
    ));
}

#[test]
fn overlapping_sensitive_terms_redact_longest_match_first() {
    let terms = BTreeSet::from(["condition".to_string(), "condition X".to_string()]);
    assert_eq!(
        generalize_query("research condition X", &terms),
        "research [redacted]"
    );
}

fn prepared_query(grant_id: Ulid, provenance: DisclosureProvenance) -> PreparedQuery {
    PreparedQuery {
        id: "prepared:test".to_string(),
        grant_id,
        action_id: ActionId::new("web.search"),
        relationship: RelationshipKind::Client,
        egress_class: EgressClass::Search,
        provenance,
        generalized_query: "research [redacted]".to_string(),
        digest: digest_of_bytes(b"placeholder"),
        created_at: jiff::Timestamp::now(),
    }
}

/// Blocker 1 regression: a prepared-query token minted under one grant
/// must never be consumable under a different requesting grant, even
/// when action/relationship/egress/provenance all otherwise match.
#[test]
fn binding_matches_rejects_a_different_requesting_grant() {
    let grant_a = Ulid::new();
    let grant_b = Ulid::new();
    let provenance = DisclosureProvenance {
        items: vec![item(DisclosureClass::Private)],
    };
    let prepared = prepared_query(grant_a, provenance.clone());
    assert!(prepared.binding_matches(
        &ActionId::new("web.search"),
        RelationshipKind::Client,
        EgressClass::Search,
        grant_a,
        &provenance,
    ));
    assert!(!prepared.binding_matches(
        &ActionId::new("web.search"),
        RelationshipKind::Client,
        EgressClass::Search,
        grant_b,
        &provenance,
    ));
}

/// Blocker 1 regression: the provenance set re-derived at consume time
/// must match exactly what the token was minted against, so a caller
/// cannot swap in a different (e.g. narrower) provenance to slip past
/// enforcement while reusing an already-redacted generalized query.
#[test]
fn binding_matches_rejects_a_different_provenance_set() {
    let grant_id = Ulid::new();
    let minted_provenance = DisclosureProvenance {
        items: vec![item(DisclosureClass::Private)],
    };
    let prepared = prepared_query(grant_id, minted_provenance);
    let other_provenance = DisclosureProvenance {
        items: vec![item(DisclosureClass::Sensitive)],
    };
    assert!(!prepared.binding_matches(
        &ActionId::new("web.search"),
        RelationshipKind::Client,
        EgressClass::Search,
        grant_id,
        &other_provenance,
    ));
}

/// The typed-identity origin is part of the minted binding (D-174): two
/// provenance sets identical in sensitivity but differing only in origin must
/// NOT bind. This mirrors `binding_matches_rejects_a_different_provenance_set`
/// and confirms `origin` participates in the derived-`PartialEq` equality the
/// consume-side re-derivation is checked against — a token minted against one
/// producing identity can never be replayed against another.
#[test]
fn binding_matches_rejects_a_different_origin() {
    let grant_id = Ulid::new();
    let minted_provenance = DisclosureProvenance {
        items: vec![item_with_origin(
            DisclosureClass::Private,
            Some(ProvenanceOrigin::Counterparty {
                identity: crate::ids::IdentityRef::from(Ulid::new()),
            }),
        )],
    };
    let prepared = prepared_query(grant_id, minted_provenance);
    let other_provenance = DisclosureProvenance {
        items: vec![item_with_origin(
            DisclosureClass::Private,
            Some(ProvenanceOrigin::Counterparty {
                identity: crate::ids::IdentityRef::from(Ulid::new()),
            }),
        )],
    };
    assert!(!prepared.binding_matches(
        &ActionId::new("web.search"),
        RelationshipKind::Client,
        EgressClass::Search,
        grant_id,
        &other_provenance,
    ));
}

#[test]
fn data_classification_maps_public_to_public() {
    assert_eq!(
        DisclosureClass::from(DataClassification::Public),
        DisclosureClass::Public
    );
}

#[test]
fn data_classification_maps_internal_to_internal() {
    assert_eq!(
        DisclosureClass::from(DataClassification::Internal),
        DisclosureClass::Internal
    );
}

#[test]
fn data_classification_maps_private_to_private() {
    assert_eq!(
        DisclosureClass::from(DataClassification::Private),
        DisclosureClass::Private
    );
}

/// Fail-closed: an unknown data classification maps to the most-restrictive
/// disclosure class so the absence of a known class is never treated as safe.
#[test]
fn data_classification_maps_unknown_to_sensitive() {
    assert_eq!(
        DisclosureClass::from(DataClassification::Unknown),
        DisclosureClass::Sensitive
    );
}

fn cp_origin(n: u128) -> ProvenanceOrigin {
    ProvenanceOrigin::Counterparty {
        identity: crate::ids::IdentityRef::from(Ulid::from(n)),
    }
}

fn cp_recipient(n: u128) -> RecipientIdentity {
    RecipientIdentity::Counterparty {
        identity: crate::ids::IdentityRef::from(Ulid::from(n)),
    }
}

fn owner_origin() -> ProvenanceOrigin {
    ProvenanceOrigin::Owner {
        principal: crate::ids::PrincipalId::from(Ulid::from(7_u128)),
    }
}

fn owner_recipient() -> RecipientIdentity {
    RecipientIdentity::Owner {
        principal: crate::ids::PrincipalId::from(Ulid::from(7_u128)),
    }
}

/// One classified item of `class` and `origin`, bound to `recipient`, over a
/// covered egress class — so the origin closure is what decides the outcome.
fn closure_query(
    origin: Option<ProvenanceOrigin>,
    recipient: RecipientIdentity,
    class: DisclosureClass,
) -> OutboundQuery {
    OutboundQuery::from_private_context(
        "hello",
        &BTreeSet::new(),
        EgressClass::Search,
        DisclosureProvenance {
            items: vec![item_with_origin(class, origin)],
        },
        recipient,
    )
}

/// D-174 / spec #220: counterparty X's datum bound for counterparty Y is
/// blocked at the origin closure even though (Client, Internal, Search) is
/// covered — the cross-counterparty leak Bell's "internal data to a stranger"
/// failure mode names.
#[test]
fn cross_counterparty_origin_is_blocked_by_the_closure() {
    let decision = check_egress(
        RelationshipKind::Client,
        closure_query(
            Some(cp_origin(1)),
            cp_recipient(2),
            DisclosureClass::Internal,
        ),
        &[policy(DisclosureClass::Internal)],
        &[],
    );
    assert!(matches!(
        decision,
        DisclosureGateDecision::CrossIdentityBlock { .. }
    ));
}

/// A counterparty's own datum reaches that same counterparty recipient.
#[test]
fn same_counterparty_origin_reaches_its_recipient() {
    let decision = check_egress(
        RelationshipKind::Client,
        closure_query(
            Some(cp_origin(1)),
            cp_recipient(1),
            DisclosureClass::Internal,
        ),
        &[policy(DisclosureClass::Internal)],
        &[],
    );
    assert!(matches!(decision, DisclosureGateDecision::Allow { .. }));
}

/// Owner-origin, non-public data cannot reach a counterparty (stranger)
/// recipient without an authorizing caveat — "internal data to a stranger"
/// closed by omission (user story 3).
#[test]
fn owner_origin_is_blocked_to_a_counterparty_recipient() {
    let decision = check_egress(
        RelationshipKind::Client,
        closure_query(
            Some(owner_origin()),
            cp_recipient(2),
            DisclosureClass::Internal,
        ),
        &[policy(DisclosureClass::Internal)],
        &[],
    );
    assert!(matches!(
        decision,
        DisclosureGateDecision::CrossIdentityBlock { .. }
    ));
}

/// Owner and system origins both reach the owner recipient.
#[test]
fn owner_and_system_origins_reach_the_owner_recipient() {
    for origin in [owner_origin(), ProvenanceOrigin::System {}] {
        let decision = check_egress(
            RelationshipKind::Client,
            closure_query(Some(origin), owner_recipient(), DisclosureClass::Internal),
            &[policy(DisclosureClass::Internal)],
            &[],
        );
        assert!(matches!(decision, DisclosureGateDecision::Allow { .. }));
    }
}

/// System-origin data is kernel-generated, never a cross-identity disclosure,
/// so it reaches any recipient.
#[test]
fn system_origin_reaches_any_recipient() {
    let decision = check_egress(
        RelationshipKind::Client,
        closure_query(
            Some(ProvenanceOrigin::System {}),
            cp_recipient(2),
            DisclosureClass::Internal,
        ),
        &[policy(DisclosureClass::Internal)],
        &[],
    );
    assert!(matches!(decision, DisclosureGateDecision::Allow { .. }));
}

/// A grant caveat that authorizes counterparty X's origin widens the closure:
/// X's datum may then egress to a different recipient (the #226 widening path).
#[test]
fn an_authorizing_caveat_widens_the_closure_for_the_named_origin() {
    let decision = check_egress(
        RelationshipKind::Client,
        closure_query(
            Some(cp_origin(1)),
            cp_recipient(2),
            DisclosureClass::Internal,
        ),
        &[policy(DisclosureClass::Internal)],
        &[cp_origin(1)],
    );
    assert!(matches!(decision, DisclosureGateDecision::Allow { .. }));
}

/// The origin closure runs ONLY after coverage passes: with no covering
/// policy the coverage stage blocks first, so a cross-identity item is never
/// masked by (nor reaches) the closure.
#[test]
fn the_closure_runs_only_after_coverage_passes() {
    let decision = check_egress(
        RelationshipKind::Client,
        closure_query(
            Some(cp_origin(1)),
            cp_recipient(2),
            DisclosureClass::Internal,
        ),
        &[],
        &[],
    );
    assert!(matches!(decision, DisclosureGateDecision::Block { .. }));
}

/// Fail closed: an unresolved (`None`) origin is most-restrictive and blocks
/// at the closure, never treated as safe to send (user story 7).
#[test]
fn an_unresolved_origin_fails_closed_at_the_closure() {
    let decision = check_egress(
        RelationshipKind::Client,
        closure_query(None, cp_recipient(2), DisclosureClass::Internal),
        &[policy(DisclosureClass::Internal)],
        &[],
    );
    assert!(matches!(
        decision,
        DisclosureGateDecision::CrossIdentityBlock { origin: None, .. }
    ));
}
