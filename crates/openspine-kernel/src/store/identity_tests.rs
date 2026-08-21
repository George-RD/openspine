#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::identity::OwnerVerifiedProof;
    use openspine_schemas::identity::{
        EntityType, Identifier, IdentifierKind, IdentifierVerificationMethod, Identity,
        Relationship, RelationshipKind,
    };
    use openspine_schemas::principal::Principal;
    use sha2::Digest as _;
    use ulid::Ulid;

    #[test]
    fn bootstrap_owner_principal_creates_exactly_one_owner_and_is_idempotent() {
        let store = Store::open_in_memory().unwrap();

        // Bootstrap first time
        let p1 = store.bootstrap_owner_principal(42, "George").unwrap();
        assert!(p1.is_owner);
        assert_eq!(p1.schema_version, 1);

        // Bootstrap second time (same parameters) - should return identical principal
        let p2 = store.bootstrap_owner_principal(42, "George").unwrap();
        assert_eq!(p1, p2);

        // Count owner principals in DB
        let count: i64 = store
            .conn
            .lock()
            .query_row(
                "SELECT COUNT(*) FROM principals WHERE is_owner = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn bootstrap_owner_principal_fails_closed_on_config_mismatch() {
        let store = Store::open_in_memory().unwrap();

        // Bootstrap first time with telegram_user_id = 42
        let _p1 = store.bootstrap_owner_principal(42, "George").unwrap();

        // Bootstrap second time with a different telegram_user_id = 99 - should fail closed (blocker)
        let res = store.bootstrap_owner_principal(99, "George");
        assert!(res.is_err());
        assert!(matches!(res.unwrap_err(), StoreError::NotOwner(_)));
    }

    #[test]
    fn database_enforces_at_most_one_owner_principal() {
        let store = Store::open_in_memory().unwrap();

        // Bootstrap first owner
        let _p1 = store.bootstrap_owner_principal(42, "George").unwrap();

        // Attempt to insert a second owner principal directly - should fail due to index constraint
        let p2 = Principal {
            id: Ulid::new(),
            identity_id: Ulid::new(),
            is_owner: true,
            schema_version: 1,
        };

        let res = store.insert_raw_principal_for_test(&p2);
        assert!(res.is_err());
    }

    #[test]
    fn owner_assert_binding_succeeds_and_is_audited_atomically() {
        let store = Store::open_in_memory().unwrap();
        let owner = store.bootstrap_owner_principal(42, "George").unwrap();

        let counterparty_id = Ulid::new();
        let mut hasher = sha2::Sha256::new();
        hasher.update(b"999");
        let val_hash = openspine_schemas::digest::digest_from_hash(hasher.finalize().into());

        let counterparty = Identity {
            id: counterparty_id,
            display_name: "Bound Counterparty".to_string(),
            entity_type: EntityType::Person,
            identifiers: vec![Identifier {
                kind: IdentifierKind::TelegramUserId,
                value_hash: val_hash.clone(),
                verified: true,
                verification_method: IdentifierVerificationMethod::UserConfirmed,
            }],
            relationships: vec![Relationship {
                kind: RelationshipKind::Spouse,
                target_id: owner.identity_id,
                confidence: 1.0,
                notes_ref: None,
            }],
            schema_version: 1,
        };

        // Assert binding
        store
            .owner_assert_identity_binding(owner.id, &OwnerVerifiedProof::test_new(), &counterparty)
            .unwrap();

        // Verify lookup resolves correctly
        let resolved = store
            .resolve_identity_by_identifier_hash(&val_hash, IdentifierKind::TelegramUserId)
            .unwrap();
        assert!(resolved.is_some());
        let res_identity = resolved.unwrap();
        assert_eq!(res_identity.id, counterparty_id);
        assert_eq!(res_identity.display_name, "Bound Counterparty");

        // Verify audit log has the identity.bound event
        let count = store.count_audit_events_of_kind("identity.bound").unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn owner_assert_binding_rolls_back_effect_on_audit_failure() {
        let store = Store::open_in_memory().unwrap();
        let owner = store.bootstrap_owner_principal(42, "George").unwrap();

        // Baseline: bootstrap created exactly the owner identity.
        let baseline_identities = store.count_identities().unwrap();
        assert_eq!(baseline_identities, 1);

        // Force the audit append for "identity.bound" to fail deterministically.
        store
            .install_audit_append_failure_for_kind("identity.bound")
            .unwrap();

        let counterparty_id = Ulid::new();
        let mut hasher = sha2::Sha256::new();
        hasher.update(b"999");
        let val_hash = openspine_schemas::digest::digest_from_hash(hasher.finalize().into());

        let counterparty = Identity {
            id: counterparty_id,
            display_name: "Bound Counterparty".to_string(),
            entity_type: EntityType::Person,
            identifiers: vec![Identifier {
                kind: IdentifierKind::TelegramUserId,
                value_hash: val_hash.clone(),
                verified: true,
                verification_method: IdentifierVerificationMethod::UserConfirmed,
            }],
            relationships: vec![Relationship {
                kind: RelationshipKind::Spouse,
                target_id: owner.identity_id,
                confidence: 1.0,
                notes_ref: None,
            }],
            schema_version: 1,
        };

        // The injected RAISE(FAIL, ...) surfaces as a Sqlite error.
        let err = store
            .owner_assert_identity_binding(owner.id, &OwnerVerifiedProof::test_new(), &counterparty)
            .unwrap_err();
        assert!(matches!(err, StoreError::Sqlite(_)));

        // AD-105: the effect rows rolled back — no orphan identity, no orphan
        // identifier mapping, and no audit row for the failed binding.
        assert!(store.get_identity(counterparty_id).unwrap().is_none());
        assert!(store
            .resolve_identity_by_identifier_hash(&val_hash, IdentifierKind::TelegramUserId)
            .unwrap()
            .is_none());
        assert_eq!(store.count_identities().unwrap(), baseline_identities);
        assert_eq!(
            store.count_audit_events_of_kind("identity.bound").unwrap(),
            0
        );
    }

    #[test]
    fn owner_assert_binding_rejects_non_owner_principal_id() {
        let store = Store::open_in_memory().unwrap();
        let _owner = store.bootstrap_owner_principal(42, "George").unwrap();

        // Create a non-owner principal record in DB
        let non_owner = Principal {
            id: Ulid::new(),
            identity_id: Ulid::new(),
            is_owner: false,
            schema_version: 1,
        };
        store.insert_raw_principal_for_test(&non_owner).unwrap();

        let counterparty = Identity {
            id: Ulid::new(),
            display_name: "Bound Counterparty".to_string(),
            entity_type: EntityType::Person,
            identifiers: vec![],
            relationships: vec![],
            schema_version: 1,
        };

        // Attempting to assert with non_owner ID should fail with NotOwner
        let res = store.owner_assert_identity_binding(
            non_owner.id,
            &OwnerVerifiedProof::test_new(),
            &counterparty,
        );
        assert!(res.is_err());
        assert!(matches!(res.unwrap_err(), StoreError::NotOwner(_)));

        // Attempting to assert with completely fake principal ID should also fail
        let res = store.owner_assert_identity_binding(
            Ulid::new(),
            &OwnerVerifiedProof::test_new(),
            &counterparty,
        );
        assert!(res.is_err());
        assert!(matches!(res.unwrap_err(), StoreError::NotOwner(_)));
    }

    #[test]
    fn principal_json_has_no_authority_fields() {
        // D-006 structural guard for Principal schema
        let principal = Principal {
            id: Ulid::new(),
            identity_id: Ulid::new(),
            is_owner: true,
            schema_version: 1,
        };
        let value = serde_json::to_value(principal).unwrap();
        let keys: Vec<&String> = value.as_object().unwrap().keys().collect();
        for forbidden in [
            "capability_pack_id",
            "task_grant_id",
            "allowed_actions",
            "route_id",
        ] {
            assert!(
                !keys.iter().any(|k| k.as_str() == forbidden),
                "principal must not carry {forbidden}"
            );
        }
    }

    /// Return the `actor` field (as a JSON value) of the single audit event of
    /// `kind`, panicking if there is not exactly one such event.
    fn actor_of_kind(store: &Store, kind: &str) -> serde_json::Value {
        let events = store.all_audit_event_jsons().unwrap();
        let mut matching: Vec<serde_json::Value> = events
            .iter()
            .map(|j| serde_json::from_str::<serde_json::Value>(j).unwrap())
            .filter(|v| v["kind"] == kind)
            .collect();
        assert_eq!(
            matching.len(),
            1,
            "expected exactly one {kind} audit event, found {}",
            matching.len()
        );
        matching.remove(0)["actor"].clone()
    }

    #[test]
    fn bootstrap_emits_bootstrapped_kind_with_owner_actor() {
        let store = Store::open_in_memory().unwrap();
        let owner = store.bootstrap_owner_principal(42, "George").unwrap();

        // Exactly one bootstrap-binding audit, carrying the new owner as actor.
        assert_eq!(
            store
                .count_audit_events_of_kind("identity.bootstrapped")
                .unwrap(),
            1
        );
        assert_eq!(
            actor_of_kind(&store, "identity.bootstrapped"),
            serde_json::json!(owner.id.to_string())
        );

        // Idempotent re-bootstrap takes the fast path and emits no new row.
        store.bootstrap_owner_principal(42, "George").unwrap();
        assert_eq!(
            store
                .count_audit_events_of_kind("identity.bootstrapped")
                .unwrap(),
            1
        );
    }

    #[test]
    fn identity_bound_carries_actor_and_no_owner_fact_in_reason() {
        let store = Store::open_in_memory().unwrap();
        let owner = store.bootstrap_owner_principal(42, "George").unwrap();

        let mut hasher = sha2::Sha256::new();
        hasher.update(b"999");
        let val_hash = openspine_schemas::digest::digest_from_hash(hasher.finalize().into());
        let counterparty = Identity {
            id: Ulid::new(),
            display_name: "Bound Counterparty".to_string(),
            entity_type: EntityType::Person,
            identifiers: vec![Identifier {
                kind: IdentifierKind::TelegramUserId,
                value_hash: val_hash,
                verified: true,
                verification_method: IdentifierVerificationMethod::UserConfirmed,
            }],
            relationships: vec![],
            schema_version: 1,
        };
        store
            .owner_assert_identity_binding(owner.id, &OwnerVerifiedProof::test_new(), &counterparty)
            .unwrap();

        // Owner fact is carried by the typed actor dimension, not the reason.
        assert_eq!(
            actor_of_kind(&store, "identity.bound"),
            serde_json::json!(owner.id.to_string())
        );
        let bound = store
            .all_audit_event_jsons()
            .unwrap()
            .into_iter()
            .map(|j| serde_json::from_str::<serde_json::Value>(&j).unwrap())
            .find(|v| v["kind"] == "identity.bound")
            .unwrap();
        let reason = bound["reason"].as_str().unwrap_or_default();
        assert!(
            !reason.contains("owner="),
            "owner fact must not remain in the reason string: {reason}"
        );
        assert!(!reason.contains(&owner.id.to_string()));
    }

    #[test]
    fn config_mismatch_emits_audit_with_actor_and_still_rejects() {
        let store = Store::open_in_memory().unwrap();
        let owner = store.bootstrap_owner_principal(42, "George").unwrap();

        // A different configured owner id fails closed but records a durable row.
        let res = store.bootstrap_owner_principal(99, "George");
        assert!(matches!(res.unwrap_err(), StoreError::NotOwner(_)));

        assert_eq!(
            store
                .count_audit_events_of_kind("identity.owner_config_mismatch")
                .unwrap(),
            1
        );
        // Actor is the stored owner principal, not the rejected config id.
        assert_eq!(
            actor_of_kind(&store, "identity.owner_config_mismatch"),
            serde_json::json!(owner.id.to_string())
        );
    }

    #[test]
    fn resolution_paths_do_not_write_audit_rows() {
        let store = Store::open_in_memory().unwrap();
        let owner = store.bootstrap_owner_principal(42, "George").unwrap();

        let before = store.all_audit_event_jsons().unwrap().len();
        let _ = store.get_identity(owner.identity_id).unwrap();
        let _ = store.principal_exists(owner.id).unwrap();
        let mut hasher = sha2::Sha256::new();
        hasher.update(b"42");
        let owner_hash = openspine_schemas::digest::digest_from_hash(hasher.finalize().into());
        let _ = store
            .resolve_identity_by_identifier_hash(&owner_hash, IdentifierKind::TelegramUserId)
            .unwrap();
        let after = store.all_audit_event_jsons().unwrap().len();

        assert_eq!(before, after, "resolution must not write audit rows");
    }

    #[test]
    fn no_raw_identifier_is_persisted_across_the_audit_dimension() {
        let store = Store::open_in_memory().unwrap();
        // Distinctive raw id, unlikely to collide with a ULID/timestamp.
        let raw = 555_000_111_222_i64;
        let raw_str = raw.to_string();
        store.bootstrap_owner_principal(raw, "George").unwrap();
        // Also exercise the mismatch path with another distinctive id.
        let raw2 = 999_888_777_665_i64;
        let raw2_str = raw2.to_string();
        let _ = store.bootstrap_owner_principal(raw2, "George");

        let conn = store.conn.lock();
        let mut stmt = conn
            .prepare("SELECT event_json, meta_json, COALESCE(kind, '') FROM audit_log")
            .unwrap();
        let rows: Vec<(String, String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        for (event_json, meta_json, _kind) in &rows {
            for raw in [&raw_str, &raw2_str] {
                assert!(
                    !event_json.contains(raw.as_str()),
                    "raw identifier {raw} leaked into event_json"
                );
                assert!(
                    !meta_json.contains(raw.as_str()),
                    "raw identifier {raw} leaked into meta_json"
                );
            }
        }
        // identity_identifiers stores only value hashes, never the raw id.
        let mut stmt = conn
            .prepare("SELECT value_hash FROM identity_identifiers")
            .unwrap();
        let hashes: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        for h in &hashes {
            assert!(!h.contains(&raw_str) && !h.contains(&raw2_str));
        }
    }
}
