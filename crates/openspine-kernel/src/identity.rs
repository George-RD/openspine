//! Identity resolver seam and binding logic (AD-146, kernel-readiness item 3).
//!
//! Resolution is pure and read-only. Counterparty binding is exposed through
//! the owner-approved, audited store API, gated on the owner-principal context.
//! Rejects owner relationship assertions.

use crate::store::{Store, StoreError};
use crate::telegram::VerifiedOwnerContext;
use openspine_schemas::event::ChannelTrust;
use openspine_schemas::identity::{
    EntityType, Identifier, IdentifierKind, IdentifierVerificationMethod, Identity,
    IdentityResolution, MatchedIdentifierType, Relationship, RelationshipKind,
};
use openspine_schemas::ids::PrincipalId;
use openspine_schemas::owner::OwnerPrincipal;
use sha2::Digest as _;
use ulid::Ulid;

pub struct IdentityResolver<'a> {
    store: &'a Store,
    owner_principal_id: Ulid,
    owner_identity_id: Ulid,
}

impl<'a> IdentityResolver<'a> {
    pub fn new(store: &'a Store, owner_principal_id: Ulid, owner_identity_id: Ulid) -> Self {
        Self {
            store,
            owner_principal_id,
            owner_identity_id,
        }
    }

    /// Resolve the kernel-owned local terminal as the configured owner.
    ///
    /// This path is intentionally separate from Telegram verification: the
    /// caller must first prove that the event was minted by the local `chat`
    /// command (`Source::Cli` + `LocalCliAuth`) and bind the owner principal.
    /// No external payload can construct this result directly.
    pub fn resolve_local_owner(
        &self,
        event_id: Ulid,
        channel_trust: ChannelTrust,
    ) -> (IdentityResolution, Option<RelationshipKind>) {
        let resolution = IdentityResolution {
            event_id,
            matched_identity_id: Some(self.owner_identity_id),
            principal_id: Some(self.owner_principal_id),
            confidence: 1.0,
            matched_identifier_type: MatchedIdentifierType::Device,
            channel_trust,
            source_verified: true,
            authority_warning: None,
            schema_version: 1,
        };
        (resolution, Some(RelationshipKind::Owner))
    }

    /// Resolve an incoming message's sender to an IdentityResolution and relationship kind.
    /// Read-only (never mutates or binds).
    pub fn resolve(
        &self,
        event_id: Ulid,
        channel_trust: ChannelTrust,
        channel_user_id: Option<&str>,
        owner_verified: Option<&VerifiedOwnerContext>,
    ) -> Result<(IdentityResolution, Option<RelationshipKind>), StoreError> {
        // 1. Explicit owner-verified path (connector-authenticated proof)
        if owner_verified.is_some() {
            let res = IdentityResolution {
                event_id,
                matched_identity_id: None,
                principal_id: Some(self.owner_principal_id),
                confidence: 1.0,
                matched_identifier_type: MatchedIdentifierType::TelegramUserId,
                channel_trust,
                source_verified: true,
                authority_warning: None,
                schema_version: 1,
            };
            return Ok((res, Some(RelationshipKind::Owner)));
        }

        // Compute sender hash if present
        let sender_hash = channel_user_id.map(|uid| {
            let mut hasher = sha2::Sha256::new();
            hasher.update(uid.as_bytes());
            openspine_schemas::digest::digest_from_hash(hasher.finalize().into())
        });

        // 2. Counterparty lookup: resolve via identity tables
        if let Some(hash) = &sender_hash {
            // Propagate DB errors (fail closed)
            if let Some(identity) = self
                .store
                .resolve_identity_by_identifier_hash(hash, IdentifierKind::TelegramUserId)?
            {
                // Find relationship targeting owner identity id
                let relationship = identity
                    .relationships
                    .iter()
                    .find(|r| r.target_id == self.owner_identity_id);

                let (kind, confidence) = match relationship {
                    Some(r) => (Some(r.kind), r.confidence),
                    None => (Some(RelationshipKind::Unknown), 0.0),
                };

                // Counterparties never yield a principal_id (AD-146)
                let res = IdentityResolution {
                    event_id,
                    matched_identity_id: Some(identity.id),
                    principal_id: None,
                    confidence,
                    matched_identifier_type: MatchedIdentifierType::TelegramUserId,
                    channel_trust,
                    source_verified: false, // counterparties are unverified in v1
                    authority_warning: None,
                    schema_version: 1,
                };
                return Ok((res, kind));
            }
        }

        // 3. Unknown identifier: resolves to Some(RelationshipKind::Unknown), confidence 0, principal_id None
        let res = IdentityResolution {
            event_id,
            matched_identity_id: None,
            principal_id: None,
            confidence: 0.0,
            matched_identifier_type: MatchedIdentifierType::None,
            channel_trust,
            source_verified: false,
            authority_warning: None,
            schema_version: 1,
        };
        Ok((res, Some(RelationshipKind::Unknown)))
    }
}

/// Process an owner-asserted counterparty binding request.
/// Returns Ok(success_message) or Err(error_message).
pub fn handle_owner_bind(
    store: &Store,
    owner_principal_id: Ulid,
    owner_identity_id: Ulid,
    proof: &VerifiedOwnerContext,
    channel_user_id: &str,
    relationship_str: &str,
) -> Result<String, String> {
    let relationship_kind = match relationship_str.to_lowercase().as_str() {
        "spouse" => RelationshipKind::Spouse,
        "colleague" => RelationshipKind::Colleague,
        "client" => RelationshipKind::Client,
        "vendor" => RelationshipKind::Vendor,
        "family" => RelationshipKind::Family,
        "owner" => {
            return Err("Owner relationship cannot be asserted".to_string());
        }
        "unknown" => {
            return Err("Unknown relationship cannot be manually asserted".to_string());
        }
        _ => {
            return Err(format!(
                "Unknown or unbindable relationship kind '{relationship_str}'"
            ));
        }
    };

    let counterparty_id = Ulid::new();
    let mut hasher = sha2::Sha256::new();
    hasher.update(channel_user_id.as_bytes());
    let value_hash = openspine_schemas::digest::digest_from_hash(hasher.finalize().into());

    let counterparty_identity = Identity {
        id: counterparty_id,
        display_name: "Bound Counterparty".to_string(),
        entity_type: EntityType::Person,
        identifiers: vec![Identifier {
            kind: IdentifierKind::TelegramUserId,
            value_hash,
            verified: true,
            verification_method: IdentifierVerificationMethod::UserConfirmed,
        }],
        relationships: vec![Relationship {
            kind: relationship_kind,
            target_id: owner_identity_id,
            confidence: 1.0,
            notes_ref: None,
        }],
        schema_version: 1,
    };

    match store.owner_assert_identity_binding(owner_principal_id, proof, &counterparty_identity) {
        Ok(()) => Ok(format!(
            "Successfully bound identifier '{channel_user_id}' as {relationship_str}"
        )),
        Err(err) => Err(format!("Failed to bind: {err}")),
    }
}

/// Bootstrap the single owner principal (AD-146) and return it as the typed
/// [`OwnerPrincipal`] aggregate (spec #197).
///
/// A thin wrapper over [`Store::bootstrap_owner_principal`]: it reuses the
/// tested, idempotent, fail-closed single-owner store machinery (never
/// reimplementing the DB insert or config-match check) and wraps the returned
/// [`Principal`] into the typed aggregate. The `telegram_user_id` passed in is
/// the same value the store hashed into the owner identity's Telegram
/// identifier, so it is the authoritative channel binding to carry on the
/// aggregate (D-002).
#[allow(dead_code)] // typed-identity foundation (spec #197 phase 1); production wiring lands with #200
pub fn bootstrap_owner_principal(
    store: &Store,
    telegram_user_id: i64,
    display_name: &str,
) -> Result<OwnerPrincipal, StoreError> {
    let principal = store.bootstrap_owner_principal(telegram_user_id, display_name)?;
    Ok(OwnerPrincipal::new(
        PrincipalId::from(principal.id),
        principal.identity_id,
        telegram_user_id,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspine_schemas::event::ChannelTrust;
    use openspine_schemas::identity::RelationshipKind;

    #[test]
    fn local_cli_path_resolves_owner_device_and_principal() {
        let store = Store::open_in_memory().unwrap();
        let owner = store.bootstrap_owner_principal(42, "George").unwrap();
        let resolver = IdentityResolver::new(&store, owner.id, owner.identity_id);
        let event_id = Ulid::new();

        let (resolution, relationship) =
            resolver.resolve_local_owner(event_id, ChannelTrust::OwnerDevice);

        assert_eq!(resolution.principal_id, Some(owner.id));
        assert_eq!(resolution.matched_identity_id, Some(owner.identity_id));
        assert_eq!(
            resolution.matched_identifier_type,
            MatchedIdentifierType::Device
        );
        assert_eq!(relationship, Some(RelationshipKind::Owner));
        assert!(resolution.source_verified);
    }

    #[test]
    fn owner_verified_path_resolves_owner_principal_and_relationship() {
        let store = Store::open_in_memory().unwrap();
        let owner = store.bootstrap_owner_principal(42, "George").unwrap();

        let resolver = IdentityResolver::new(&store, owner.id, owner.identity_id);
        let event_id = Ulid::new();
        let (res, rel) = resolver
            .resolve(
                event_id,
                ChannelTrust::VerifiedOwnerChannel,
                None,
                Some(&VerifiedOwnerContext::test_new()),
            )
            .unwrap();

        assert_eq!(res.principal_id, Some(owner.id));
        assert_eq!(res.matched_identity_id, None);
        assert_eq!(rel, Some(RelationshipKind::Owner));
        assert!(res.source_verified);
    }

    #[test]
    fn counterparty_resolves_identity_but_no_principal() {
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

        store
            .owner_assert_identity_binding(
                owner.id,
                &VerifiedOwnerContext::test_new(),
                &counterparty,
            )
            .unwrap();

        let resolver = IdentityResolver::new(&store, owner.id, owner.identity_id);
        let event_id = Ulid::new();

        // Resolve counterparty by user ID
        let (res, rel) = resolver
            .resolve(
                event_id,
                ChannelTrust::VerifiedOwnerChannel,
                Some("999"),
                None,
            )
            .unwrap();
        assert_eq!(res.principal_id, None);
        assert_eq!(res.matched_identity_id, Some(counterparty_id));
        assert_eq!(rel, Some(RelationshipKind::Spouse));
        assert!(!res.source_verified);
    }

    #[test]
    fn unknown_resolves_to_relationship_unknown_confidence_0_and_no_write() {
        let store = Store::open_in_memory().unwrap();
        let owner = store.bootstrap_owner_principal(42, "George").unwrap();

        let resolver = IdentityResolver::new(&store, owner.id, owner.identity_id);
        let event_id = Ulid::new();

        // Count identities before
        let count_before = store.count_identities().unwrap();

        // Resolve unknown
        let (res, rel) = resolver
            .resolve(
                event_id,
                ChannelTrust::VerifiedOwnerChannel,
                Some("999"),
                None,
            )
            .unwrap();
        assert_eq!(res.principal_id, None);
        assert_eq!(res.matched_identity_id, None);
        assert_eq!(rel, Some(RelationshipKind::Unknown));
        assert_eq!(res.confidence, 0.0);
        assert!(!res.source_verified);

        // Count identities after - must be unchanged (no write)
        let count_after = store.count_identities().unwrap();
        assert_eq!(count_before, count_after);
    }

    #[test]
    fn bootstrap_owner_principal_wrapper_is_idempotent() {
        let store = Store::open_in_memory().unwrap();

        let first = bootstrap_owner_principal(&store, 42, "George").unwrap();
        let second = bootstrap_owner_principal(&store, 42, "George").unwrap();

        assert_eq!(first, second);
        // The store's single-owner invariant (AD-146) is enforced and tested
        // at the store layer; here we confirm the persisted owner is the one
        // the wrapper returned.
        let stored = store.owner_principal().unwrap().unwrap();
        assert_eq!(first.principal_id, PrincipalId::from(stored.id));
    }

    #[test]
    fn bootstrap_owner_principal_wrapper_fails_closed_on_config_mismatch() {
        let store = Store::open_in_memory().unwrap();

        bootstrap_owner_principal(&store, 42, "George").unwrap();
        let mismatch = bootstrap_owner_principal(&store, 99, "George");

        assert!(matches!(mismatch, Err(StoreError::NotOwner(_))));
    }

    #[test]
    fn bootstrap_owner_principal_wrapper_maps_fields_from_store() {
        let store = Store::open_in_memory().unwrap();

        let owner = bootstrap_owner_principal(&store, 42, "George").unwrap();
        let stored = store.owner_principal().unwrap().unwrap();

        assert_eq!(owner.principal_id, PrincipalId::from(stored.id));
        assert_eq!(owner.identity_id, stored.identity_id);
        assert_eq!(owner.telegram_binding(), 42);
    }
}
