// Store-owned read-only census for package-transition quiescence (#285).
//
// This file is included from `package_install.rs` so package maintenance owns
// the boundary without exposing a raw SQLite connection outside Store.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum OutstandingWorkSource {
    TaskGrants,
    WorkflowSteps,
    WorkerDispatches,
    WorkflowTimers,
    TaskDispatchQueue,
    DependencyWaiters,
    StandingRulePendingActions,
    StandingRuleReservations,
    WorkerResultRelays,
    OwnerNotificationQueue,
    ActionRequests,
    EffectFences,
}

impl OutstandingWorkSource {
    pub(crate) const ALL: [Self; 12] = [
        Self::TaskGrants,
        Self::WorkflowSteps,
        Self::WorkerDispatches,
        Self::WorkflowTimers,
        Self::TaskDispatchQueue,
        Self::DependencyWaiters,
        Self::StandingRulePendingActions,
        Self::StandingRuleReservations,
        Self::WorkerResultRelays,
        Self::OwnerNotificationQueue,
        Self::ActionRequests,
        Self::EffectFences,
    ];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::TaskGrants => "task_grants",
            Self::WorkflowSteps => "workflow_step_registry",
            Self::WorkerDispatches => "worker_dispatch",
            Self::WorkflowTimers => "workflow_timers",
            Self::TaskDispatchQueue => "dispatch_state",
            Self::DependencyWaiters => "task_dependency_waiters",
            Self::StandingRulePendingActions => "standing_rule_pending_actions",
            Self::StandingRuleReservations => "standing_rule_usage",
            Self::WorkerResultRelays => "worker_result_relays",
            Self::OwnerNotificationQueue => "notify_dead_letters",
            Self::ActionRequests => "action_requests",
            Self::EffectFences => "pending_draft_writes",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct OutstandingWorkCounts {
    pub(crate) outstanding: u64,
    pub(crate) terminal: u64,
    pub(crate) unknown: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackageOutstandingWork {
    entries: Vec<(OutstandingWorkSource, OutstandingWorkCounts)>,
}

impl PackageOutstandingWork {
    pub(crate) fn source(&self, source: OutstandingWorkSource) -> OutstandingWorkCounts {
        self.entries
            .iter()
            .find_map(|(candidate, counts)| (*candidate == source).then_some(*counts))
            .unwrap_or_default()
    }

    pub(crate) fn is_quiescent(&self) -> bool {
        OutstandingWorkSource::ALL.into_iter().all(|source| {
            let counts = self.source(source);
            counts.outstanding == 0 && counts.unknown == 0
        })
    }
}

impl Store {
    /// Read one transactionally-consistent view of every persisted work family
    /// that can still execute, resume, retry, or authorize an effect under the
    /// currently configured base. The census never recovers, consumes,
    /// cancels, settles, or otherwise mutates those rows.
    ///
    /// Every row must land in exactly one of three buckets: outstanding,
    /// terminal history, or unknown. Unknown is intentionally not normalized
    /// away: callers treat it as non-quiescent so schema/state drift cannot
    /// silently make a package transition look safe.
    pub(crate) fn package_outstanding_work(
        &self,
        as_of: Timestamp,
    ) -> Result<PackageOutstandingWork, StoreError> {
        let as_of = super::sql_timestamp(as_of);
        self.with_deferred_read(|tx| {
            let mut entries = Vec::with_capacity(OutstandingWorkSource::ALL.len());
            for source in OutstandingWorkSource::ALL {
                let (total, outstanding, terminal) = source_counts(tx, source, &as_of)?;
                let total = u64::try_from(total).map_err(|_| StoreError::NumericRange)?;
                let outstanding =
                    u64::try_from(outstanding).map_err(|_| StoreError::NumericRange)?;
                let terminal = u64::try_from(terminal).map_err(|_| StoreError::NumericRange)?;
                let classified = outstanding
                    .checked_add(terminal)
                    .ok_or(StoreError::NumericRange)?;
                let unknown = total.checked_sub(classified).ok_or_else(|| {
                    StoreError::BadLedgerMeta(format!(
                        "package outstanding-work census overlaps for {}",
                        source.as_str()
                    ))
                })?;
                entries.push((
                    source,
                    OutstandingWorkCounts {
                        outstanding,
                        terminal,
                        unknown,
                    },
                ));
            }
            Ok(PackageOutstandingWork { entries })
        })
    }
}

fn source_counts(
    tx: &rusqlite::Transaction<'_>,
    source: OutstandingWorkSource,
    as_of: &str,
) -> Result<(i64, i64, i64), StoreError> {
    let counts = match source {
        OutstandingWorkSource::TaskGrants => tx.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN expires_at > ?1 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN expires_at <= ?1 THEN 1 ELSE 0 END), 0)
             FROM task_grants",
            [as_of],
            count_row,
        )?,
        OutstandingWorkSource::WorkflowSteps => tx.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN completed_seq IS NULL OR completed_seq = -1 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN completed_seq >= 0 THEN 1 ELSE 0 END), 0)
             FROM workflow_step_registry",
            [],
            count_row,
        )?,
        OutstandingWorkSource::WorkerDispatches => tx.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN state = 'dispatched' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN state = 'terminal' THEN 1 ELSE 0 END), 0)
             FROM worker_dispatch",
            [],
            count_row,
        )?,
        OutstandingWorkSource::WorkflowTimers => tx.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN status = 'fired' THEN 1 ELSE 0 END), 0)
             FROM workflow_timers",
            [],
            count_row,
        )?,
        OutstandingWorkSource::TaskDispatchQueue => tx.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN state IN ('pending', 'handed_off') THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN state = 'terminal' THEN 1 ELSE 0 END), 0)
             FROM dispatch_state",
            [],
            count_row,
        )?,
        OutstandingWorkSource::DependencyWaiters => tx.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN state IN ('waiting', 'ready') THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN state = 'consumed' THEN 1 ELSE 0 END), 0)
             FROM task_dependency_waiters",
            [],
            count_row,
        )?,
        OutstandingWorkSource::StandingRulePendingActions => tx.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE
                        WHEN resolved_at IS NULL AND resolution IS NULL AND dispatch_state = 'none' THEN 1
                        WHEN resolved_at IS NOT NULL AND resolution = 'allowed'
                             AND dispatch_state IN ('none', 'claimed') THEN 1
                        ELSE 0 END), 0),
                    COALESCE(SUM(CASE
                        WHEN resolved_at IS NOT NULL AND resolution IN ('denied', 'stale')
                             AND dispatch_state = 'none' THEN 1
                        WHEN resolved_at IS NOT NULL AND resolution = 'allowed'
                             AND dispatch_state = 'dispatched' THEN 1
                        ELSE 0 END), 0)
             FROM standing_rule_pending_actions",
            [],
            count_row,
        )?,
        OutstandingWorkSource::StandingRuleReservations => tx.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN status = 'reserved' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN status IN ('committed', 'waiver') THEN 1 ELSE 0 END), 0)
             FROM standing_rule_usage",
            [],
            count_row,
        )?,
        OutstandingWorkSource::WorkerResultRelays => tx.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN state IN ('attempting', 'pending') THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN state IN ('delivered', 'skipped', 'dead_letter') THEN 1 ELSE 0 END), 0)
             FROM worker_result_relays",
            [],
            count_row,
        )?,
        OutstandingWorkSource::OwnerNotificationQueue => tx.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN state IN ('pending', 'in_progress') THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN state = 'resolved' THEN 1 ELSE 0 END), 0)
             FROM notify_dead_letters",
            [],
            count_row,
        )?,
        OutstandingWorkSource::ActionRequests => tx.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN used = 0 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN used = 1 THEN 1 ELSE 0 END), 0)
             FROM action_requests",
            [],
            count_row,
        )?,
        OutstandingWorkSource::EffectFences => tx.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN state = 'pending' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN state = 'resolved' THEN 1 ELSE 0 END), 0)
             FROM pending_draft_writes",
            [],
            count_row,
        )?,
    };
    Ok(counts)
}

fn count_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(i64, i64, i64)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert_outstanding(store: &Store, source: OutstandingWorkSource, as_of: Timestamp) {
        store.with_conn_for_test(|conn| match source {
            OutstandingWorkSource::TaskGrants => {
                conn.execute(
                    "INSERT INTO task_grants
                     (id, task_token, expires_at, grant_json, pending_message_digest, owner_surface_json)
                     VALUES ('grant-live', 'token-live', ?1, '{}', 'sha256:00', '{}')",
                    [crate::store::sql_timestamp(
                        as_of + std::time::Duration::from_secs(60),
                    )],
                )
                .unwrap();
            }
            OutstandingWorkSource::WorkflowSteps => {
                conn.execute(
                    "INSERT INTO workflow_step_registry
                     (run_id, step_id, pending_seq, completed_seq)
                     VALUES ('run-live', 'step-live', 1, NULL)",
                    [],
                )
                .unwrap();
            }
            OutstandingWorkSource::WorkerDispatches => {
                conn.execute(
                    "INSERT INTO worker_dispatch
                     (grant_id, parent_grant_id, state, receipt_key, request_digest, token_ref, created_at, updated_at)
                     VALUES ('worker-live', 'parent-live', 'dispatched', 'receipt-live', 'digest-live', '', 'now', 'now')",
                    [],
                )
                .unwrap();
            }
            OutstandingWorkSource::WorkflowTimers => {
                conn.execute(
                    "INSERT INTO workflow_timers
                     (timer_id, run_id, fires_at, status, fired_event_id)
                     VALUES ('timer-live', 'run-timer-live', 1, 'pending', NULL)",
                    [],
                )
                .unwrap();
            }
            OutstandingWorkSource::TaskDispatchQueue => {
                conn.execute(
                    "INSERT INTO dispatch_state
                     (event_id, timer_id, state, created_at, updated_at)
                     VALUES ('event-live', 'task-timer-live', 'handed_off', 'now', 'now')",
                    [],
                )
                .unwrap();
            }
            OutstandingWorkSource::DependencyWaiters => {
                conn.execute(
                    "INSERT INTO task_dependency_waiters
                     (task_id, owner_principal_id, dependency_id, timer_id, event_id, state, created_at, updated_at)
                     VALUES ('task-live', 'owner-live', 'dep-live', 'dep-timer-live', 'dep-event-live', 'waiting', 'now', 'now')",
                    [],
                )
                .unwrap();
            }
            OutstandingWorkSource::StandingRulePendingActions => {
                conn.execute(
                    "INSERT INTO standing_rule_pending_actions
                     (pending_id, rule_id, rule_version, task_grant_id, action_id,
                      dark_window_default, request_fingerprint, requested_at, resolved_at,
                      resolution, dispatch_state)
                     VALUES ('pending-live', 'rule-live', 1, 'grant-live', 'email.create_draft',
                             'allow', 'fingerprint-live', 1, NULL, NULL, 'none')",
                    [],
                )
                .unwrap();
            }
            OutstandingWorkSource::StandingRuleReservations => {
                conn.execute(
                    "INSERT INTO standing_rule_usage
                     (rule_id, version, kind, used_at, status, reservation_id)
                     VALUES ('rule-res-live', 1, 'quota', 1, 'reserved', 'reservation-live')",
                    [],
                )
                .unwrap();
            }
            OutstandingWorkSource::WorkerResultRelays => {
                conn.execute(
                    "INSERT INTO worker_result_relays
                     (event_id, global_seq, task_grant_id, state, attempts, created_at, updated_at)
                     VALUES ('relay-live', 1, NULL, 'pending', 1, 'now', 'now')",
                    [],
                )
                .unwrap();
            }
            OutstandingWorkSource::OwnerNotificationQueue => {
                conn.execute(
                    "INSERT INTO notify_dead_letters
                     (id, enqueued_at, owner_surface_json, text_ref, task_grant_id,
                      digest_item_ids, attempts, next_attempt_at, state)
                     VALUES ('notify-live', 'now', '{}', 'sha256:notify', NULL, '', 0, 'now', 'pending')",
                    [],
                )
                .unwrap();
            }
            OutstandingWorkSource::ActionRequests => {
                conn.execute(
                    "INSERT INTO action_requests (id, request_json, used)
                     VALUES ('request-live', '{}', 0)",
                    [],
                )
                .unwrap();
            }
            OutstandingWorkSource::EffectFences => {
                conn.execute(
                    "INSERT INTO pending_draft_writes
                     (id, grant_id, action_request_id, thread_id, request_fingerprint, created_at, state)
                     VALUES ('effect-live', 'grant-live', 'request-live', 'thread-live', 'effect-fp-live', 'now', 'pending')",
                    [],
                )
                .unwrap();
            }
        });
    }

    fn insert_terminal(store: &Store, source: OutstandingWorkSource, as_of: Timestamp) {
        store.with_conn_for_test(|conn| match source {
            OutstandingWorkSource::TaskGrants => {
                conn.execute(
                    "INSERT INTO task_grants
                     (id, task_token, expires_at, grant_json, pending_message_digest, owner_surface_json)
                     VALUES ('grant-old', 'token-old', ?1, '{}', 'sha256:00', '{}')",
                    [crate::store::sql_timestamp(
                        as_of - std::time::Duration::from_secs(60),
                    )],
                )
                .unwrap();
            }
            OutstandingWorkSource::WorkflowSteps => {
                conn.execute(
                    "INSERT INTO workflow_step_registry
                     (run_id, step_id, pending_seq, completed_seq)
                     VALUES ('run-old', 'step-old', 1, 2)",
                    [],
                )
                .unwrap();
            }
            OutstandingWorkSource::WorkerDispatches => {
                conn.execute(
                    "INSERT INTO worker_dispatch
                     (grant_id, parent_grant_id, state, receipt_key, request_digest, token_ref, created_at, updated_at)
                     VALUES ('worker-old', 'parent-old', 'terminal', 'receipt-old', 'digest-old', '', 'now', 'now')",
                    [],
                )
                .unwrap();
            }
            OutstandingWorkSource::WorkflowTimers => {
                conn.execute(
                    "INSERT INTO workflow_timers
                     (timer_id, run_id, fires_at, status, fired_event_id)
                     VALUES ('timer-old', 'run-timer-old', 1, 'fired', 'fired-event')",
                    [],
                )
                .unwrap();
            }
            OutstandingWorkSource::TaskDispatchQueue => {
                conn.execute(
                    "INSERT INTO dispatch_state
                     (event_id, timer_id, state, created_at, updated_at)
                     VALUES ('event-old', 'task-timer-old', 'terminal', 'now', 'now')",
                    [],
                )
                .unwrap();
            }
            OutstandingWorkSource::DependencyWaiters => {
                conn.execute(
                    "INSERT INTO task_dependency_waiters
                     (task_id, owner_principal_id, dependency_id, timer_id, event_id, state, created_at, updated_at)
                     VALUES ('task-old', 'owner-old', 'dep-old', 'dep-timer-old', 'dep-event-old', 'consumed', 'now', 'now')",
                    [],
                )
                .unwrap();
            }
            OutstandingWorkSource::StandingRulePendingActions => {
                conn.execute(
                    "INSERT INTO standing_rule_pending_actions
                     (pending_id, rule_id, rule_version, task_grant_id, action_id,
                      dark_window_default, request_fingerprint, requested_at, resolved_at,
                      resolution, dispatch_state)
                     VALUES ('pending-old', 'rule-old', 1, 'grant-old', 'email.create_draft',
                             'deny', 'fingerprint-old', 1, 2, 'denied', 'none')",
                    [],
                )
                .unwrap();
            }
            OutstandingWorkSource::StandingRuleReservations => {
                conn.execute(
                    "INSERT INTO standing_rule_usage
                     (rule_id, version, kind, used_at, status, reservation_id)
                     VALUES ('rule-res-old', 1, 'quota', 1, 'committed', 'reservation-old')",
                    [],
                )
                .unwrap();
            }
            OutstandingWorkSource::WorkerResultRelays => {
                conn.execute(
                    "INSERT INTO worker_result_relays
                     (event_id, global_seq, task_grant_id, state, attempts, created_at, updated_at)
                     VALUES ('relay-old', 1, NULL, 'delivered', 1, 'now', 'now')",
                    [],
                )
                .unwrap();
            }
            OutstandingWorkSource::OwnerNotificationQueue => {
                conn.execute(
                    "INSERT INTO notify_dead_letters
                     (id, enqueued_at, owner_surface_json, text_ref, task_grant_id,
                      digest_item_ids, attempts, next_attempt_at, state)
                     VALUES ('notify-old', 'now', '{}', 'sha256:notify-old', NULL, '', 1, 'now', 'resolved')",
                    [],
                )
                .unwrap();
            }
            OutstandingWorkSource::ActionRequests => {
                conn.execute(
                    "INSERT INTO action_requests (id, request_json, used)
                     VALUES ('request-old', '{}', 1)",
                    [],
                )
                .unwrap();
            }
            OutstandingWorkSource::EffectFences => {
                conn.execute(
                    "INSERT INTO pending_draft_writes
                     (id, grant_id, action_request_id, thread_id, request_fingerprint, created_at, state, resolved_at)
                     VALUES ('effect-old', 'grant-old', 'request-old', 'thread-old', 'effect-fp-old', 'now', 'resolved', 'later')",
                    [],
                )
                .unwrap();
            }
        });
    }

    #[test]
    fn empty_store_is_quiescent() {
        let store = Store::open_in_memory().unwrap();
        let snapshot = store.package_outstanding_work(Timestamp::now()).unwrap();
        assert!(snapshot.is_quiescent());
        for source in OutstandingWorkSource::ALL {
            assert_eq!(snapshot.source(source), OutstandingWorkCounts::default());
        }
    }

    #[test]
    fn every_enumerated_outstanding_family_blocks_quiescence() {
        let as_of = Timestamp::now();
        for source in OutstandingWorkSource::ALL {
            let store = Store::open_in_memory().unwrap();
            insert_outstanding(&store, source, as_of);
            let snapshot = store.package_outstanding_work(as_of).unwrap();
            assert_eq!(
                snapshot.source(source),
                OutstandingWorkCounts {
                    outstanding: 1,
                    terminal: 0,
                    unknown: 0,
                },
                "{} must be classified as outstanding",
                source.as_str()
            );
            assert!(!snapshot.is_quiescent(), "{} must block", source.as_str());
        }
    }

    #[test]
    fn terminal_or_completed_history_does_not_block() {
        let as_of = Timestamp::now();
        for source in OutstandingWorkSource::ALL {
            let store = Store::open_in_memory().unwrap();
            insert_terminal(&store, source, as_of);
            let snapshot = store.package_outstanding_work(as_of).unwrap();
            assert_eq!(
                snapshot.source(source),
                OutstandingWorkCounts {
                    outstanding: 0,
                    terminal: 1,
                    unknown: 0,
                },
                "{} terminal history must be recognized",
                source.as_str()
            );
            assert!(
                snapshot.is_quiescent(),
                "{} terminal history must not block",
                source.as_str()
            );
        }
    }

    #[test]
    fn unrecognized_persisted_state_blocks_fail_closed() {
        let store = Store::open_in_memory().unwrap();
        store.with_conn_for_test(|conn| {
            conn.execute(
                "INSERT INTO action_requests (id, request_json, used)
                 VALUES ('request-unknown', '{}', 2)",
                [],
            )
            .unwrap();
        });
        let snapshot = store.package_outstanding_work(Timestamp::now()).unwrap();
        assert_eq!(
            snapshot.source(OutstandingWorkSource::ActionRequests),
            OutstandingWorkCounts {
                outstanding: 0,
                terminal: 0,
                unknown: 1,
            }
        );
        assert!(!snapshot.is_quiescent());
    }
}
