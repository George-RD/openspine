use super::*;
use crate::digest::Digest;
fn item(class: DisclosureClass) -> ClassifiedBriefcaseItem {
    ClassifiedBriefcaseItem {
        item_ref: ArtifactRef {
            digest: Digest::parse(
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            )
            .expect("digest"),
            schema_version: 1,
        },
        disclosure_class: class,
    }
}

fn query(class: DisclosureClass) -> OutboundQuery {
    OutboundQuery::from_private_context(
        "research condition X",
        &BTreeSet::from(["condition X".to_string()]),
        EgressClass::Search,
        DisclosureProvenance {
            items: vec![item(class)],
        },
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
        check_egress(RelationshipKind::Client, outbound, &[]),
        DisclosureGateDecision::Block { .. }
    ));
}

#[test]
fn active_relationship_and_class_policy_allows_covered_egress() {
    assert!(matches!(
        check_egress(
            RelationshipKind::Client,
            query(DisclosureClass::Private),
            &[policy(DisclosureClass::Private)]
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
            &[]
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
            &[covered]
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
