//! Storage backing grant-level runtime budgets (`GrantLimits.max_model_calls`
//! / `max_artifacts`) and stale-grant cleanup (`harden-approval-and-budgets`,
//! D-046/D-047). Split out of `store/mod.rs` to keep that file under the
//! 500-line gate — these queries are conceptually one group (everything this
//! change added) separate from the task-grant/audit-log core.

use jiff::Timestamp;
use rusqlite::params;
use ulid::Ulid;

use super::{Store, StoreError};

/// 24h grant-retention window for [`Store::sweep_expired_grants`] —
/// comfortably past the ≤180s selection-token/approval TTLs already in use
/// elsewhere in the kernel, so nothing live is ever at risk of being swept.
const GRANT_RETENTION: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// D-047: `task_grants.task_token` stores a hash of the bearer token, never
/// the plaintext — the column name is unchanged (a rename needs a full
/// SQLite table rebuild) but its content is `sha256:<hex>` of the token.
/// Plain equality at the SQL layer is fine: tokens are 32 random bytes with
/// no realistic timing-attack surface worth constant-time comparison.
pub(super) fn hash_task_token(token: &str) -> String {
    openspine_schemas::digest::digest_of_bytes(token.as_bytes()).to_string()
}

impl Store {
    // ---- model-call budget (D-046) ---------------------------------------

    /// Atomically consume one unit of `grant_id`'s model-call budget.
    /// `post_model_generate` calls this once per request — a `max` of `N`
    /// allows exactly `N` calls: the Nth call finds `model_calls == N - 1`,
    /// increments and allows; the `N + 1`th finds `model_calls == N` and is
    /// denied without incrementing further.
    ///
    /// One SQL statement, same TOCTOU-avoidance rationale as
    /// `try_count_artifact_put` below: the `ON CONFLICT` branch's own
    /// `WHERE` clause is the single point of decision, so two concurrent
    /// requests racing for the same last call can never both pass. The
    /// prior implementation counted `conversation_state` rows for role
    /// `"user"` with a plain `SELECT COUNT` compared in application code —
    /// correct sequentially, but two concurrent requests could both read
    /// the same pre-increment count and both be allowed, exceeding the
    /// budget. Found in review; no test previously exercised concurrency.
    pub fn try_count_model_call(&self, grant_id: Ulid, max: u32) -> Result<bool, StoreError> {
        if max == 0 {
            return Ok(false);
        }
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO grant_counters (grant_id, model_calls) VALUES (?1, 1)
             ON CONFLICT(grant_id) DO UPDATE SET model_calls = model_calls + 1
             WHERE model_calls < ?2",
            params![grant_id.to_string(), max],
        )?;
        Ok(conn.changes() == 1)
    }

    // ---- artifact-put budget (D-046) --------------------------------------

    /// Atomically consume one unit of `grant_id`'s artifact-put budget.
    /// Counts only shell-initiated blob puts (`model.generate`'s payload
    /// snapshot, `propose_draft_creation`'s draft payload) — never internal
    /// kernel bookkeeping blobs like conversation turns, which would
    /// otherwise collide with the default `max_artifacts: 20` limit.
    ///
    /// One SQL statement, same TOCTOU-avoidance rationale as
    /// [`super::gate_support`]'s `try_consume_selection_token`: the `ON
    /// CONFLICT` branch's own `WHERE` clause is the single point of
    /// decision, so two concurrent callers racing for the same last slot
    /// can never both observe room for it.
    pub fn try_count_artifact_put(&self, grant_id: Ulid, max: u32) -> Result<bool, StoreError> {
        if max == 0 {
            // The unconditional first-insert branch below ignores `max` (it
            // has nothing to compare against yet) — a zero-budget grant must
            // still get zero puts, not one "free" one.
            return Ok(false);
        }
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO grant_counters (grant_id, artifact_puts) VALUES (?1, 1)
             ON CONFLICT(grant_id) DO UPDATE SET artifact_puts = artifact_puts + 1
             WHERE artifact_puts < ?2",
            params![grant_id.to_string(), max],
        )?;
        Ok(conn.changes() == 1)
    }

    // ---- expired-grant sweep (D-047) --------------------------------------

    /// Delete grants (and their counters) that expired more than 24 hours
    /// ago. Called at the top of `insert_task_grant` — no separate scheduled
    /// job exists yet, so every new grant is itself a sweep trigger.
    pub fn sweep_expired_grants(&self, now: Timestamp) -> Result<(), StoreError> {
        let cutoff = (now - GRANT_RETENTION).to_string();
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM task_grants WHERE expires_at < ?1",
            params![cutoff],
        )?;
        conn.execute(
            "DELETE FROM grant_counters WHERE grant_id NOT IN (SELECT id FROM task_grants)",
            [],
        )?;
        Ok(())
    }
}
