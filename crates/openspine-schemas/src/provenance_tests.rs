//! Invariant tests for [`ProvenanceOrigin`]: typed construction, serde
//! round-trip, D-006 (no authority field), and `deny_unknown_fields`.

use super::*;
use ulid::Ulid;

fn sample_origins() -> Vec<ProvenanceOrigin> {
    vec![
        ProvenanceOrigin::Owner {
            principal: PrincipalId::from(Ulid::new()),
        },
        ProvenanceOrigin::Counterparty {
            identity: IdentityRef::from(Ulid::new()),
        },
        ProvenanceOrigin::System {},
    ]
}

#[test]
fn owner_binds_typed_principal_id_not_a_raw_ulid() {
    let ulid = Ulid::new();
    let origin = ProvenanceOrigin::Owner {
        principal: PrincipalId::from(ulid),
    };
    // The owner arm carries a typed PrincipalId (#197), recoverable as its
    // inner Ulid — proof it is not a bare string / raw Ulid field.
    match origin {
        ProvenanceOrigin::Owner { principal } => assert_eq!(principal.as_ulid(), ulid),
        other => panic!("expected Owner, got {other:?}"),
    }
}

#[test]
fn counterparty_binds_typed_identity_ref() {
    let ulid = Ulid::new();
    let origin = ProvenanceOrigin::Counterparty {
        identity: IdentityRef::from(ulid),
    };
    match origin {
        ProvenanceOrigin::Counterparty { identity } => assert_eq!(identity.as_ulid(), ulid),
        other => panic!("expected Counterparty, got {other:?}"),
    }
}

#[test]
fn each_variant_round_trips_through_serde() {
    for origin in sample_origins() {
        let json = serde_json::to_string(&origin).unwrap();
        let back: ProvenanceOrigin = serde_json::from_str(&json).unwrap();
        assert_eq!(origin, back);
    }
}

#[test]
fn internal_tag_wire_shape_is_kind_plus_origin_id_only() {
    let ulid = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();

    let owner = serde_json::to_value(ProvenanceOrigin::Owner {
        principal: PrincipalId::from(ulid),
    })
    .unwrap();
    assert_eq!(
        owner,
        serde_json::json!({ "kind": "owner", "principal": "01ARZ3NDEKTSV4RRFFQ69G5FAV" })
    );

    let counterparty = serde_json::to_value(ProvenanceOrigin::Counterparty {
        identity: IdentityRef::from(ulid),
    })
    .unwrap();
    assert_eq!(
        counterparty,
        serde_json::json!({ "kind": "counterparty", "identity": "01ARZ3NDEKTSV4RRFFQ69G5FAV" })
    );

    let system = serde_json::to_value(ProvenanceOrigin::System {}).unwrap();
    assert_eq!(system, serde_json::json!({ "kind": "system" }));
}

#[test]
fn no_variant_carries_an_authority_field() {
    // Structural proof for D-006: an origin records identity only. Every
    // serialized variant's keys are a subset of {kind, principal, identity},
    // and none of the authority-shaped keys ever appears.
    for origin in sample_origins() {
        let value = serde_json::to_value(&origin).unwrap();
        let keys: Vec<String> = value
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.to_string())
            .collect();
        for key in &keys {
            assert!(
                matches!(key.as_str(), "kind" | "principal" | "identity"),
                "unexpected origin key {key} in {origin:?}"
            );
        }
        for forbidden in [
            "capability_pack_id",
            "task_grant_id",
            "allowed_actions",
            "route_id",
        ] {
            assert!(
                !keys.iter().any(|k| k == forbidden),
                "origin {origin:?} must not carry {forbidden}"
            );
        }
    }
}

#[test]
fn deny_unknown_fields_rejects_injected_authority_field_on_every_variant() {
    // Every variant must reject an injected authority-shaped key, System
    // included: a serde internally-tagged *unit* variant silently ignores
    // unknown keys (fail-open), so System is an empty struct variant to keep
    // the whole closed enum fail-closed (D-006).
    for origin in sample_origins() {
        let mut value = serde_json::to_value(&origin).unwrap();
        value.as_object_mut().unwrap().insert(
            "capability_pack_id".into(),
            serde_json::json!("owner_control_basic_pack"),
        );
        assert!(
            serde_json::from_value::<ProvenanceOrigin>(value).is_err(),
            "deny_unknown_fields must reject an authority-shaped key on {origin:?}"
        );
    }
}

#[test]
fn unknown_kind_fails_closed() {
    let value = serde_json::json!({ "kind": "delegated", "principal": Ulid::new().to_string() });
    assert!(
        serde_json::from_value::<ProvenanceOrigin>(value).is_err(),
        "an unknown origin kind must not deserialize (closed enum, fail-closed)"
    );
}
