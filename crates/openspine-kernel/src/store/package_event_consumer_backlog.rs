use crate::store::event_bus::PersistedConsumerState;
use openspine_schemas::audit::AuditKind;
use openspine_schemas::event_bus::EventSubscriptionFilter;
use openspine_schemas::nerve::{NerveDeclaration, NerveType};
use rusqlite::OptionalExtension;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckpointState {
    Valid(u64),
    Invalid,
}

fn event_consumer_backlog_counts(
    tx: &rusqlite::Transaction<'_>,
) -> Result<(i64, i64, i64), StoreError> {
    let static_consumers = [
        (
            "worker_result_consumer",
            EventSubscriptionFilter::kinds([AuditKind::from_static("worker.result")]),
        ),
        (
            "worker_failed_consumer",
            EventSubscriptionFilter::kinds([AuditKind::from_static("worker.failed")]),
        ),
        (
            "task_board_timer_consumer",
            EventSubscriptionFilter::kinds([AuditKind::from_static("workflow.timer_fired")]),
        ),
        (
            "standing_rule_dark_window_consumer",
            EventSubscriptionFilter::kinds([AuditKind::from_static("workflow.timer_fired")]),
        ),
    ];

    let mut outstanding = 0_i64;
    let mut invalid_bindings = 0_i64;
    for (consumer_id, filter) in static_consumers {
        match checkpoint_state(tx, consumer_id, &filter, false)? {
            CheckpointState::Valid(after) => {
                outstanding = checked_add_i64(
                    outstanding,
                    matching_backlog(tx, &filter, after)?,
                )?;
            }
            CheckpointState::Invalid => invalid_bindings = checked_add_i64(invalid_bindings, 1)?,
        }
    }

    let registrations = tx
        .prepare("SELECT nerve_id, declaration_json FROM nerve_registrations")?
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    for (nerve_id, declaration_json) in registrations {
        let declaration: NerveDeclaration = match serde_json::from_str(&declaration_json) {
            Ok(value) => value,
            Err(_) => {
                invalid_bindings = checked_add_i64(invalid_bindings, 1)?;
                continue;
            }
        };
        if declaration.schema_version != 1
            || declaration.subscription_filter.schema_version != 1
            || declaration.id.to_string() != nerve_id
        {
            invalid_bindings = checked_add_i64(invalid_bindings, 1)?;
            continue;
        }
        // Only Screener nerves have a production dispatcher today. Other
        // registered nerve types are configuration, not replayable work.
        if declaration.nerve_type != NerveType::Screener {
            continue;
        }
        let consumer_id = format!("nerve:{nerve_id}");
        match checkpoint_state(
            tx,
            &consumer_id,
            &declaration.subscription_filter,
            true,
        )? {
            CheckpointState::Valid(after) => {
                outstanding = checked_add_i64(
                    outstanding,
                    matching_backlog(tx, &declaration.subscription_filter, after)?,
                )?;
            }
            CheckpointState::Invalid => invalid_bindings = checked_add_i64(invalid_bindings, 1)?,
        }
    }

    let total = checked_add_i64(outstanding, invalid_bindings)?;
    Ok((total, outstanding, 0))
}

fn checkpoint_state(
    tx: &rusqlite::Transaction<'_>,
    consumer_id: &str,
    expected_filter: &EventSubscriptionFilter,
    required: bool,
) -> Result<CheckpointState, StoreError> {
    let row: Option<(i64, String)> = tx
        .query_row(
            "SELECT last_acked_global_seq, checkpoint_json
             FROM consumer_checkpoints WHERE consumer_id = ?1",
            [consumer_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((stored_seq, checkpoint_json)) = row else {
        return Ok(if required {
            CheckpointState::Invalid
        } else {
            CheckpointState::Valid(0)
        });
    };
    let Ok(stored_seq) = u64::try_from(stored_seq) else {
        return Ok(CheckpointState::Invalid);
    };
    let state: PersistedConsumerState = match serde_json::from_str(&checkpoint_json) {
        Ok(value) => value,
        Err(_) => return Ok(CheckpointState::Invalid),
    };
    if state.schema_version != 1
        || state.checkpoint.schema_version != 1
        || state.filter.schema_version != 1
        || state.filter != *expected_filter
        || state.checkpoint.last_acked_global_seq != stored_seq
    {
        return Ok(CheckpointState::Invalid);
    }
    Ok(CheckpointState::Valid(stored_seq))
}

fn matching_backlog(
    tx: &rusqlite::Transaction<'_>,
    filter: &EventSubscriptionFilter,
    after: u64,
) -> Result<i64, StoreError> {
    let after = i64::try_from(after).map_err(|_| StoreError::NumericRange)?;
    let count = Store::replay_audit_conn(tx, filter, after)?.len();
    i64::try_from(count).map_err(|_| StoreError::NumericRange)
}

fn checked_add_i64(left: i64, right: i64) -> Result<i64, StoreError> {
    left.checked_add(right).ok_or(StoreError::NumericRange)
}

#[cfg(test)]
mod continuation_tests {
    include!("package_work_continuation_tests.rs");
}
