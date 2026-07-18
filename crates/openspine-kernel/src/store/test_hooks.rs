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

    /// Open a Store against an already-initialized DB file in READ-ONLY mode, so
    /// the next write (e.g. `append_audit`) fails with a genuine IO/readonly
    /// SQLite error — the disk-full/IO class (AD-139). The file MUST already have
    /// the schema (open it normally first, then drop, then reopen read-only).
    pub(crate) fn open_read_only_for_test(
        path: &std::path::Path,
    ) -> Result<super::Store, super::StoreError> {
        use rusqlite::OpenFlags;
        let conn = rusqlite::Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        Ok(super::Store {
            conn: std::sync::Arc::new(parking_lot::Mutex::new(conn)),
            #[cfg(test)]
            activation_tx_failure: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            #[cfg(test)]
            fault_init_tx: std::sync::Arc::new(std::sync::Mutex::new(false)),
            #[cfg(test)]
            fail_next_skill_promotion_tx: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
                false,
            )),
            fail_next_owner_reconfirmation: std::sync::Arc::new(
                std::sync::atomic::AtomicBool::new(false),
            ),
            fail_next_standing_rule_remaining: std::sync::Arc::new(
                std::sync::atomic::AtomicBool::new(false),
            ),
            fail_next_effective_allow_audit: std::sync::Arc::new(
                std::sync::atomic::AtomicBool::new(false),
            ),
        })
    }

    /// Test-only access to the store's single shared connection. Used by
    /// the literal disk-full (SQLITE_FULL) proof: it clamps
    /// `PRAGMA max_page_count` on the real database file's connection and
    /// saturates the audit ledger, so the production append path is driven
    /// against a genuinely full database. The connection is an
    /// `Arc<Mutex<Connection>>` shared by every `Store` clone, so
    /// saturating it here is visible to the server's store too. The
    /// closure must release the lock (return) before any store method that
    /// re-locks, to avoid self-deadlock.
    pub(crate) fn with_conn_for_test<R>(&self, f: impl FnOnce(&rusqlite::Connection) -> R) -> R {
        let conn = self.conn.lock();
        f(&conn)
    }
}
