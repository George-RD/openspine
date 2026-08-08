use super::Store;
use crate::store::StoreError;

impl Store {
    /// Install a per-store, kind-targeted SQLite fault for deterministic
    /// action-level tests. Each in-memory Store owns its connection, so this
    /// does not use process-global mutable state or cross-test coordination.
    #[allow(dead_code)]
    pub fn install_audit_append_failure_for_kind(&self, kind: &str) -> Result<(), StoreError> {
        let escaped = kind.replace('\'', "''");
        let conn = self.conn.lock();
        conn.execute_batch(&format!(
            "CREATE TRIGGER fail_audit_append_{suffix} \
             BEFORE INSERT ON audit_log WHEN NEW.kind = '{escaped}' \
             BEGIN SELECT RAISE(FAIL, 'injected audit append failure'); END;",
            suffix = escaped.replace(|c: char| !c.is_ascii_alphanumeric(), "_"),
        ))?;
        Ok(())
    }
}
