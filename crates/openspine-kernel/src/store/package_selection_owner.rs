//! Read-only owner/instance evidence for package selection (ANO-822).
use super::{Store, StoreError};
use openspine_schemas::identity::IdentifierKind;
use openspine_schemas::principal::Principal;
use sha2::Digest as _;

impl Store {
    /// The persisted owner, verified against configuration without writing.
    ///
    /// Unlike bootstrap this never inserts a principal, appends an audit row or
    /// rebinds anything: package maintenance must not mint or repair identity.
    /// Absence, a missing identity or a different configured owner all fail.
    pub(crate) fn verified_owner_for_selection(
        &self,
        telegram_user_id: i64,
    ) -> Result<Principal, StoreError> {
        let owner = self
            .owner_principal()?
            .ok_or_else(|| StoreError::NotOwner("no persisted owner principal".into()))?;
        let identity = self
            .get_identity(owner.identity_id)?
            .ok_or_else(|| StoreError::NotOwner("persisted owner identity is missing".into()))?;
        let mut hasher = sha2::Sha256::new();
        hasher.update(telegram_user_id.to_string().as_bytes());
        let configured = openspine_schemas::digest::digest_from_hash(hasher.finalize().into());
        let matches = identity.id == owner.identity_id
            && identity.identifiers.iter().any(|identifier| {
                identifier.kind == IdentifierKind::TelegramUserId
                    && identifier.value_hash == configured
            });
        if matches {
            Ok(owner)
        } else {
            Err(StoreError::NotOwner(
                "persisted owner does not match the configured owner".into(),
            ))
        }
    }
}
