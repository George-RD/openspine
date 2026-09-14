use super::{LedgerEntry, StoreError};
use openspine_schemas::audit::AuditEvent;
use openspine_schemas::event_bus::EventSubscriptionFilter;
use rusqlite::{params, Connection};

/// One shared validator for materialized replay and count-only observation.
/// Kind filtering deliberately follows validation: a corrupted nonmatching
/// event must not disappear merely because the caller only needs a count.
pub(super) fn visit(
    conn: &Connection,
    filter: &EventSubscriptionFilter,
    after_global_seq: i64,
    mut emit: impl FnMut(LedgerEntry) -> Result<(), StoreError>,
) -> Result<(), StoreError> {
    let (sql, aggregate) = match filter.aggregate_id.as_deref() {
        Some(aggregate) => (
            "SELECT seq, event_json, meta_json, id, kind, aggregate_id, aggregate_seq, prev_hash, hash FROM audit_log \
             WHERE seq > ?1 AND aggregate_id = ?2 ORDER BY seq ASC",
            Some(aggregate),
        ),
        None => (
            "SELECT seq, event_json, meta_json, id, kind, aggregate_id, aggregate_seq, prev_hash, hash FROM audit_log \
             WHERE seq > ?1 ORDER BY seq ASC",
            None,
        ),
    };
    let mut statement = conn.prepare(sql)?;
    let mut rows = match aggregate {
        Some(aggregate) => statement.query(params![after_global_seq, aggregate])?,
        None => statement.query(params![after_global_seq])?,
    };
    while let Some(row) = rows.next()? {
        let entry = validated_entry(row)?;
        if filter.matches(&entry.event.kind, &entry.event.aggregate_id) {
            emit(entry)?;
        }
    }
    Ok(())
}

impl super::Store {
    /// Check an exact nonzero acknowledgement against the same projection
    /// validator as replay. A point lookup rejects future coordinates and
    /// gaps without scanning or retaining the preceding ledger history.
    pub(crate) fn audit_checkpoint_matches_conn(
        conn: &Connection,
        filter: &EventSubscriptionFilter,
        global_seq: i64,
    ) -> Result<bool, StoreError> {
        if global_seq <= 0 {
            return Ok(false);
        }
        let mut statement = conn.prepare(
            "SELECT seq, event_json, meta_json, id, kind, aggregate_id, aggregate_seq, prev_hash, hash
             FROM audit_log WHERE seq = ?1",
        )?;
        let mut rows = statement.query([global_seq])?;
        let Some(row) = rows.next()? else {
            return Ok(false);
        };
        let entry = validated_entry(row)?;
        Ok(filter.matches(&entry.event.kind, &entry.event.aggregate_id))
    }
}

fn validated_entry(row: &rusqlite::Row<'_>) -> Result<LedgerEntry, StoreError> {
    let seq: i64 = row.get(0)?;
    let event_json: String = row.get(1)?;
    let meta_json: String = row.get(2)?;
    let row_id: String = row.get(3)?;
    let row_kind: String = row.get(4)?;
    let row_aggregate: String = row.get(5)?;
    let row_aggregate_seq: i64 = row.get(6)?;
    let row_prev_hash: String = row.get(7)?;
    let row_hash: String = row.get(8)?;
    let meta: serde_json::Value = serde_json::from_str(&meta_json)?;
    let meta_id = meta.get("id").and_then(|v| v.as_str()).unwrap_or_default();
    let meta_kind = meta
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let meta_aggregate = meta
        .get("aggregate_id")
        .and_then(|v| v.as_str())
        .unwrap_or("system");
    let meta_seq = meta
        .get("aggregate_seq")
        .and_then(|v| v.as_u64())
        .unwrap_or_default();
    if meta_id != row_id
        || meta_kind != row_kind
        || meta_aggregate != row_aggregate
        || meta_seq != row_aggregate_seq as u64
    {
        return Err(StoreError::BadLedgerMeta(format!(
            "ledger row {seq} metadata mismatch"
        )));
    }
    let event: AuditEvent = serde_json::from_str(&event_json)?;
    if event.schema_version != 1
        || event.id.to_string() != row_id
        || event.kind.as_str() != row_kind
        || event.aggregate_id != row_aggregate
        || event.aggregate_seq != row_aggregate_seq as u64
        || event.prev_hash.as_str() != row_prev_hash
        || event.hash.as_str() != row_hash
    {
        return Err(StoreError::BadLedgerMeta(format!(
            "ledger row {seq} event_json mismatch"
        )));
    }
    // event_json is a redundant cache, never an authority. Preserve every
    // field check and legacy default from the original replay implementation.
    let event_value = serde_json::to_value(&event)?;
    for field in [
        "id",
        "ts",
        "kind",
        "action",
        "decision",
        "reason",
        "task_grant_id",
        "target_refs",
        "payload_refs",
        "aggregate_id",
        "aggregate_seq",
        "payload_json",
        "actor",
    ] {
        let normalized = match field {
            "aggregate_id" => meta
                .get(field)
                .cloned()
                .unwrap_or_else(|| serde_json::json!("system")),
            "aggregate_seq" => meta
                .get(field)
                .cloned()
                .unwrap_or_else(|| serde_json::json!(0)),
            _ => meta.get(field).cloned().unwrap_or(serde_json::Value::Null),
        };
        if event_value.get(field) != Some(&normalized) {
            return Err(StoreError::BadLedgerMeta(format!(
                "ledger row {seq} event_json field {field} mismatch"
            )));
        }
    }
    Ok(LedgerEntry {
        global_seq: seq as u64,
        event,
    })
}
