//! SQLite storage identity and principal operations (AD-146).
//!
//! Enforces D-006 (identity has zero authority fields) and the single-owner
//! invariant at the database layer using a partial unique index.

use super::{Store, StoreError};
use crate::telegram::VerifiedOwnerContext;
use openspine_schemas::digest::Digest;
use openspine_schemas::identity::{
    EntityType, Identifier, IdentifierKind, IdentifierVerificationMethod, Identity,
};
use openspine_schemas::principal::Principal;
use rusqlite::{params, OptionalExtension};
use sha2::Digest as _;
use ulid::Ulid;

impl Store {
    /// Idempotently establish the single owner principal and identity.
    /// Transactional and unique-constraint safe. Fails closed if the DB
    /// owner does not match current configured owner Telegram user ID.
    pub fn bootstrap_owner_principal(
        &self,
        telegram_user_id: i64,
        display_name: &str,
    ) -> Result<Principal, StoreError> {
        // Idempotent fast path: existing owner must match current config.
        if let Some(owner) = self.owner_principal()? {
            return self.ensure_owner_matches_config(&owner, telegram_user_id);
        }

        match self.try_insert_owner_principal(telegram_user_id, display_name) {
            Ok(principal) => Ok(principal),
            Err(err) if is_unique_constraint_store_error(&err) => {
                // Lost a concurrent bootstrap race (identifier or principal
                // unique violation). Re-read the winner and re-run the
                // config-match check so a different Telegram id never
                // silently attaches to another owner's principal.
                let owner = self.owner_principal()?.ok_or_else(|| {
                    StoreError::NotOwner(
                        "Owner principal missing after concurrent bootstrap race".into(),
                    )
                })?;
                self.ensure_owner_matches_config(&owner, telegram_user_id)
            }
            Err(err) => Err(err),
        }
    }

    /// Fail closed unless the stored owner identity's Telegram identifier
    /// hash matches the currently configured owner Telegram user id.
    fn ensure_owner_matches_config(
        &self,
        owner: &Principal,
        telegram_user_id: i64,
    ) -> Result<Principal, StoreError> {
        let owner_identity = self
            .get_identity(owner.identity_id)?
            .ok_or_else(|| StoreError::NotOwner("Owner identity missing in DB".into()))?;

        let mut hasher = sha2::Sha256::new();
        hasher.update(telegram_user_id.to_string().as_bytes());
        let current_hash = openspine_schemas::digest::digest_from_hash(hasher.finalize().into());

        let has_matching_identifier = owner_identity.identifiers.iter().any(|ident| {
            ident.kind == IdentifierKind::TelegramUserId && ident.value_hash == current_hash
        });

        if !has_matching_identifier {
            return Err(StoreError::NotOwner(
                "Stored owner Telegram identifier does not match current configured owner ID"
                    .to_string(),
            ));
        }

        Ok(owner.clone())
    }

    /// Insert a fresh owner principal + identity. Unique-constraint races
    /// (identifier PK or single-owner partial index) surface as Sqlite errors
    /// for the caller to recover from.
    fn try_insert_owner_principal(
        &self,
        telegram_user_id: i64,
        display_name: &str,
    ) -> Result<Principal, StoreError> {
        let mut hasher = sha2::Sha256::new();
        hasher.update(telegram_user_id.to_string().as_bytes());
        let owner_hash = openspine_schemas::digest::digest_from_hash(hasher.finalize().into());

        let identity_id = Ulid::new();
        let owner_identity = Identity {
            id: identity_id,
            display_name: display_name.to_string(),
            entity_type: EntityType::Person,
            identifiers: vec![Identifier {
                kind: IdentifierKind::TelegramUserId,
                value_hash: owner_hash.clone(),
                verified: true,
                verification_method: IdentifierVerificationMethod::SetupPairing,
            }],
            relationships: vec![],
            schema_version: 1,
        };

        let principal = Principal {
            id: Ulid::new(),
            identity_id,
            is_owner: true,
            schema_version: 1,
        };

        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;

        tx.execute(
            "INSERT INTO identities (id, identity_json) VALUES (?1, ?2)",
            params![
                identity_id.to_string(),
                serde_json::to_string(&owner_identity)?
            ],
        )?;

        tx.execute(
            "INSERT INTO identity_identifiers (value_hash, identifier_kind, identity_id) VALUES (?1, ?2, ?3)",
            params![
                owner_hash.as_str(),
                "telegram_user_id",
                identity_id.to_string()
            ],
        )?;

        let principal_json = serde_json::to_string(&principal)?;
        tx.execute(
            "INSERT INTO principals (id, identity_id, is_owner, principal_json) VALUES (?1, ?2, ?3, ?4)",
            params![
                principal.id.to_string(),
                principal.identity_id.to_string(),
                1,
                principal_json
            ],
        )?;

        tx.commit()?;
        Ok(principal)
    }

    /// Read the single owner principal from the DB.
    pub fn owner_principal(&self) -> Result<Option<Principal>, StoreError> {
        let conn = self.conn.lock();
        let row: Option<String> = conn
            .query_row(
                "SELECT principal_json FROM principals WHERE is_owner = 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let Some(json) = row else {
            return Ok(None);
        };
        let principal: Principal = serde_json::from_str(&json)?;
        Ok(Some(principal))
    }

    /// Look up a principal by ID and verify it is the owner.
    pub fn owner_principal_by_id(&self, principal_id: Ulid) -> Result<Principal, StoreError> {
        let conn = self.conn.lock();
        let row: Option<(String, i64)> = conn
            .query_row(
                "SELECT principal_json, is_owner FROM principals WHERE id = ?1",
                params![principal_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((json, is_owner)) = row else {
            return Err(StoreError::NotOwner(format!(
                "Principal {principal_id} not found"
            )));
        };
        if is_owner != 1 {
            return Err(StoreError::NotOwner(format!(
                "Principal {principal_id} is not owner"
            )));
        }
        let principal: Principal = serde_json::from_str(&json)?;
        Ok(principal)
    }
    /// Existence-only principal check (finding 5). Returns `Ok(false)` when no
    /// principal row matches (a permanent, non-retryable condition), and
    /// `Err` only on a transient store failure — so the caller may safely
    /// treat `Ok(false)` as "reject" while still retrying on `Err`.
    #[allow(dead_code)]
    pub fn principal_exists(&self, principal_id: Ulid) -> Result<bool, StoreError> {
        let conn = self.conn.lock();
        let exists: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM principals WHERE id = ?1",
                params![principal_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(exists.is_some())
    }

    /// Owner-asserted relationship binding ("my wife's number is this").
    /// Gated on an authenticated owner-principal context and audited atomically in the transaction.
    pub fn owner_assert_identity_binding(
        &self,
        owner_principal_id: Ulid,
        _proof: &VerifiedOwnerContext,
        identity: &Identity,
    ) -> Result<(), StoreError> {
        // Enforce owner-principal context boundary
        self.owner_principal_by_id(owner_principal_id)?;

        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;

        // Insert identity
        tx.execute(
            "INSERT INTO identities (id, identity_json) VALUES (?1, ?2)",
            params![identity.id.to_string(), serde_json::to_string(identity)?],
        )?;

        // Insert identifiers mapping
        for identifier in &identity.identifiers {
            let kind_str = match identifier.kind {
                IdentifierKind::TelegramUserId => "telegram_user_id",
                IdentifierKind::Email => "email",
                IdentifierKind::WhatsappNumber => "whatsapp_number",
            };
            tx.execute(
                "INSERT INTO identity_identifiers (value_hash, identifier_kind, identity_id) VALUES (?1, ?2, ?3)",
                params![
                    identifier.value_hash.as_str(),
                    kind_str,
                    identity.id.to_string()
                ],
            )?;
        }

        // Blocker audit alignment: append audit record INSIDE the transaction
        // before commit, so binding and audit are atomic.
        Self::append_audit_conn(
            &tx,
            "identity.bound",
            None,
            None,
            Some(&format!(
                "owner={owner_principal_id} identity={}",
                identity.id
            )),
            None,
            &[],
            &[],
        )?;

        tx.commit()?;
        Ok(())
    }

    /// Retrieve an identity record by ID.
    pub fn get_identity(&self, id: Ulid) -> Result<Option<Identity>, StoreError> {
        let conn = self.conn.lock();
        let json: Option<String> = conn
            .query_row(
                "SELECT identity_json FROM identities WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        let Some(identity_str) = json else {
            return Ok(None);
        };
        let identity: Identity = serde_json::from_str(&identity_str)?;
        Ok(Some(identity))
    }

    /// Read-only identifier resolution lookup.
    pub fn resolve_identity_by_identifier_hash(
        &self,
        value_hash: &Digest,
        kind: IdentifierKind,
    ) -> Result<Option<Identity>, StoreError> {
        let kind_str = match kind {
            IdentifierKind::TelegramUserId => "telegram_user_id",
            IdentifierKind::Email => "email",
            IdentifierKind::WhatsappNumber => "whatsapp_number",
        };
        let conn = self.conn.lock();
        let identity_id: Option<String> = conn
            .query_row(
                "SELECT identity_id FROM identity_identifiers WHERE value_hash = ?1 AND identifier_kind = ?2",
                params![value_hash.as_str(), kind_str],
                |row| row.get(0),
            )
            .optional()?;
        let Some(id) = identity_id else {
            return Ok(None);
        };
        let json: Option<String> = conn
            .query_row(
                "SELECT identity_json FROM identities WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(identity_str) = json else {
            return Ok(None);
        };
        let identity: Identity = serde_json::from_str(&identity_str)?;
        Ok(Some(identity))
    }

    /// Helper for testing
    #[cfg(test)]
    pub fn count_identities(&self) -> Result<usize, StoreError> {
        let conn = self.conn.lock();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM identities", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Helper for testing single-owner unique constraints
    #[cfg(test)]
    pub fn insert_raw_principal_for_test(&self, principal: &Principal) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO principals (id, identity_id, is_owner, principal_json) VALUES (?1, ?2, ?3, ?4)",
            params![
                principal.id.to_string(),
                principal.identity_id.to_string(),
                if principal.is_owner { 1 } else { 0 },
                serde_json::to_string(principal)?
            ],
        )?;
        Ok(())
    }
}

fn is_unique_constraint_violation(err: &rusqlite::Error) -> bool {
    match err {
        rusqlite::Error::SqliteFailure(ffi_err, Some(msg)) => {
            ffi_err.code == rusqlite::ErrorCode::ConstraintViolation
                && (msg.contains("UNIQUE")
                    || msg.contains("unique")
                    || msg.contains("constraint failed"))
        }
        _ => false,
    }
}

fn is_unique_constraint_store_error(err: &StoreError) -> bool {
    match err {
        StoreError::Sqlite(e) => is_unique_constraint_violation(e),
        _ => false,
    }
}
