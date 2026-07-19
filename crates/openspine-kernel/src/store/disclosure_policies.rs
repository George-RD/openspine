//! Per-scope disclosure policy storage.

use jiff::Timestamp;
use openspine_schemas::digest::Digest;
use openspine_schemas::disclosure_policy::{DisclosurePolicy, PreparedQuery, PreparedQueryRef};
use openspine_schemas::egress::EgressClass;
use openspine_schemas::identity::RelationshipKind;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::to_string as to_json;
use ulid::Ulid;

use super::{Store, StoreError};

/// A durable owner-question awaiting an allow/deny answer. The blocked query
/// digest is kernel-derived at block time; an owner command can only select
/// this pending question by id and cannot supply an arbitrary digest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DisclosurePendingQuestion {
    pub pending_id: Ulid,
    pub grant_id: Ulid,
    pub relationship: RelationshipKind,
    pub disclosure_class: openspine_schemas::disclosure_policy::DisclosureClass,
    pub egress_class: EgressClass,
    pub blocked_query_digest: Digest,
    pub created_at: Timestamp,
}

pub(super) fn ensure_schema(conn: &rusqlite::Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS disclosure_policies (
            id TEXT PRIMARY KEY,
            policy_json TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS prepared_queries (
            id TEXT PRIMARY KEY,
            query_json TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS disclosure_pending_questions (
            pending_id TEXT PRIMARY KEY,
            question_json TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );",
    )?;
    Ok(())
}

impl Store {
    pub fn store_disclosure_policy(
        &self,
        policy: &DisclosurePolicy,
        now: Timestamp,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        let json = to_json(policy)?;
        conn.execute(
            "INSERT INTO disclosure_policies (id, policy_json, created_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET policy_json = excluded.policy_json",
            params![policy.id, json, now.as_second()],
        )?;
        Ok(())
    }

    pub fn load_disclosure_policies(&self) -> Result<Vec<DisclosurePolicy>, StoreError> {
        let conn = self.conn.lock();
        let rows: Vec<String> = conn
            .prepare("SELECT policy_json FROM disclosure_policies")?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|json| {
                serde_json::from_str(&json).map_err(|e| {
                    StoreError::ProposedArtifactLifecycle(format!("disclosure policy corrupt: {e}"))
                })
            })
            .collect()
    }

    pub fn store_prepared_query(&self, prepared: &PreparedQuery) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        let json = to_json(prepared)?;
        conn.execute(
            "INSERT INTO prepared_queries (id, query_json, created_at)
             VALUES (?1, ?2, ?3)",
            params![prepared.id, json, prepared.created_at.as_second()],
        )?;
        Ok(())
    }

    pub fn consume_prepared_query(
        &self,
        r#ref: &PreparedQueryRef,
    ) -> Result<Option<PreparedQuery>, StoreError> {
        let conn = self.conn.lock();
        let json: Option<String> = conn
            .query_row(
                "SELECT query_json FROM prepared_queries WHERE id = ?1",
                params![r#ref.id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(json) = json else {
            return Ok(None);
        };
        let prepared: PreparedQuery = serde_json::from_str(&json).map_err(|e| {
            StoreError::ProposedArtifactLifecycle(format!("prepared query corrupt: {e}"))
        })?;
        if prepared.digest != r#ref.digest || prepared.digest != prepared.binding_digest() {
            return Ok(None);
        }
        conn.execute(
            "DELETE FROM prepared_queries WHERE id = ?1",
            params![r#ref.id],
        )?;
        Ok(Some(prepared))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn store_disclosure_pending_question(
        &self,
        pending_id: &Ulid,
        grant_id: Ulid,
        relationship: RelationshipKind,
        disclosure_class: openspine_schemas::disclosure_policy::DisclosureClass,
        egress_class: EgressClass,
        blocked_query_digest: Digest,
        now: Timestamp,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        let question = DisclosurePendingQuestion {
            pending_id: *pending_id,
            grant_id,
            relationship,
            disclosure_class,
            egress_class,
            blocked_query_digest,
            created_at: now,
        };
        let json = to_json(&question)?;
        conn.execute(
            "INSERT INTO disclosure_pending_questions (pending_id, question_json, created_at)
             VALUES (?1, ?2, ?3)",
            params![pending_id.to_string(), json, now.as_second()],
        )?;
        Ok(())
    }

    pub(crate) fn load_disclosure_pending_question(
        &self,
        pending_id: &Ulid,
    ) -> Result<Option<DisclosurePendingQuestion>, StoreError> {
        let conn = self.conn.lock();
        let json: Option<String> = conn
            .query_row(
                "SELECT question_json FROM disclosure_pending_questions WHERE pending_id = ?1",
                params![pending_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        let Some(json) = json else {
            return Ok(None);
        };
        serde_json::from_str(&json).map(Some).map_err(|e| {
            StoreError::ProposedArtifactLifecycle(format!("pending question corrupt: {e}"))
        })
    }

    pub fn resolve_disclosure_pending_question(
        &self,
        pending_id: &Ulid,
        _now: Timestamp,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM disclosure_pending_questions WHERE pending_id = ?1",
            params![pending_id.to_string()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::fixtures::test_state;
    use openspine_schemas::artifact::Lifecycle;
    use openspine_schemas::disclosure_policy::{
        DisclosureCarveOut, DisclosureClass, DisclosurePolicyKey,
    };

    #[test]
    fn disclosure_policy_round_trips_through_store() {
        let state = test_state();
        let now = Timestamp::now();
        let policy = DisclosurePolicy {
            id: "disclosure:client:private".to_string(),
            schema_version: 1,
            version: 1,
            lifecycle_state: Lifecycle::Active,
            key: DisclosurePolicyKey {
                relationship: RelationshipKind::Client,
                disclosure_class: DisclosureClass::Private,
            },
            allowed_egress_classes: vec![EgressClass::Search],
            standing_rule_bindings: {
                let mut map = std::collections::BTreeMap::new();
                map.insert(
                    EgressClass::Search,
                    "disclosure:client:private:search".to_string(),
                );
                map
            },
            carve_outs: vec![DisclosureCarveOut {
                egress_class: EgressClass::Search,
                query_shape: openspine_schemas::digest::digest_of_bytes(b"research [redacted]"),
            }],
        };
        state.store.store_disclosure_policy(&policy, now).unwrap();
        let loaded = state.store.load_disclosure_policies().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].carve_outs, policy.carve_outs);
        assert_eq!(loaded[0].key, policy.key);
    }
}
