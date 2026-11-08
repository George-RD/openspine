//! Fingerprint of live authority-control state for package selection.
//!
//! A selection decision binds this alongside the overlay evaluation: lifecycle,
//! pause, revocation, evaluation, erasure, budget, consumption and effect-fence
//! rows all live in Store tables. Rather than enumerate them (and silently miss
//! a later one), every table is covered except the hash-chained audit log and
//! the selection bookkeeping that this very command writes. An unrelated audit
//! append therefore never stales a review, but a changed control row always does.
use rusqlite::types::ValueRef;
use sha2::{Digest as _, Sha256};

use super::{Store, StoreError};
use openspine_schemas::digest::{digest_from_hash, Digest};

const EXCLUDED: [&str; 4] = [
    "audit_log",
    "package_selection",
    "package_selection_history",
    "package_selection_decisions",
];
const MAX_ROWS: usize = 2_000_000;

fn absorb(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

impl Store {
    pub(crate) fn package_control_fingerprint(&self) -> Result<Digest, StoreError> {
        self.with_deferred_read(|tx| {
            let tables = tx
                .prepare(
                    "SELECT name FROM sqlite_master
                     WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
                )?
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            let mut hasher = Sha256::new();
            hasher.update(b"openspine:package-control-state:v1\0");
            let mut rows_seen = 0usize;
            for table in tables
                .iter()
                .filter(|name| !EXCLUDED.contains(&name.as_str()))
            {
                let quoted = format!("\"{}\"", table.replace('"', "\"\""));
                // WITHOUT ROWID tables have no rowid; they iterate in key order.
                let mut statement =
                    match tx.prepare(&format!("SELECT * FROM {quoted} ORDER BY rowid")) {
                        Ok(statement) => statement,
                        Err(_) => tx.prepare(&format!("SELECT * FROM {quoted}"))?,
                    };
                let columns = statement.column_count();
                absorb(&mut hasher, table.as_bytes());
                let mut rows = statement.query([])?;
                while let Some(row) = rows.next()? {
                    rows_seen += 1;
                    if rows_seen > MAX_ROWS {
                        return Err(StoreError::NumericRange);
                    }
                    hasher.update(b"\x01");
                    for index in 0..columns {
                        match row.get_ref(index)? {
                            ValueRef::Null => hasher.update(b"n"),
                            ValueRef::Integer(value) => {
                                hasher.update(b"i");
                                hasher.update(value.to_le_bytes());
                            }
                            ValueRef::Real(value) => {
                                hasher.update(b"r");
                                hasher.update(value.to_bits().to_le_bytes());
                            }
                            ValueRef::Text(bytes) => {
                                hasher.update(b"t");
                                absorb(&mut hasher, bytes);
                            }
                            ValueRef::Blob(bytes) => {
                                hasher.update(b"b");
                                absorb(&mut hasher, bytes);
                            }
                        }
                    }
                }
            }
            Ok(digest_from_hash(hasher.finalize().into()))
        })
    }
}
