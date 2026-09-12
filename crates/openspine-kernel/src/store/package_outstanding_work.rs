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
    pub(crate) fn package_outstanding_work(
        &self,
        _as_of: Timestamp,
    ) -> Result<PackageOutstandingWork, StoreError> {
        // TDD RED placeholder: the tests below require persisted state to be
        // classified. Returning an empty census is deliberately wrong.
        Ok(PackageOutstandingWork {
            entries: OutstandingWorkSource::ALL
                .into_iter()
                .map(|source| (source, OutstandingWorkCounts::default()))
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

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
                params![],
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
