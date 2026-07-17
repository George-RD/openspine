//! Test-only hooks on [`super::Store`]: transaction-fault injection (to prove
//! the startup bot-id/legacy-offset migration rolls back atomically) and
//! introspection helpers for the secret-leak proofs. Lives in its own module
//! so it does not count toward `store/mod.rs`'s 500-line gate.

impl super::Store {
    /// Drop the `audit_log` table so the next `append_audit` fails.
    /// Used to verify rollback semantics in secret-intake.
    pub(crate) fn break_audit_for_test(&self) {
        let conn = self.conn.lock();
        let _ = conn.execute_batch("DROP TABLE audit_log");
    }

    /// Drop connector counters so the next counter write fails.
    pub(crate) fn break_connector_counters_for_test(&self) {
        let conn = self.conn.lock();
        let _ = conn.execute_batch("DROP TABLE connector_counters");
    }

    /// Arm a one-shot fault so the next
    /// `initialize_telegram_bot_id_and_migrate_offset` fails its SQLite
    /// transaction (and rolls back atomically). Consumed on fire so a retry
    /// re-attempts cleanly.
    pub(crate) fn arm_fault_init_tx_for_test(&self) {
        *self.fault_init_tx.lock().expect("fault_init_tx poisoned") = true;
    }

    /// Return every `kv_state` row `(key, value)`. Used to prove a captured
    /// secret never lands in any kernel-persisted, shell/observer-visible
    /// key/value surface.
    pub(crate) fn all_kv_for_test(&self) -> Vec<(String, String)> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare("SELECT key, value FROM kv_state ORDER BY key")
            .expect("prepare kv scan");
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .expect("query kv scan");
        rows.map(|r| r.expect("row")).collect()
    }
}
