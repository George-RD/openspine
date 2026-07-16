#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::telegram::VerifiedOwnerContext;
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
            .owner_assert_identity_binding(
                owner.id,
                &VerifiedOwnerContext::test_new(),
                &counterparty,
            )
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
            &VerifiedOwnerContext::test_new(),
            &counterparty,
        );
        assert!(res.is_err());
        assert!(matches!(res.unwrap_err(), StoreError::NotOwner(_)));

        // Attempting to assert with completely fake principal ID should also fail
        let res = store.owner_assert_identity_binding(
            Ulid::new(),
            &VerifiedOwnerContext::test_new(),
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
}
