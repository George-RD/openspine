// Store-owned read-only census for package-transition quiescence (#285).
//
// This file is included inside package maintenance so the Store owns the
// boundary without exposing a raw SQLite connection or inventing a runtime
// caller before the package-review model is wired.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum OutstandingWorkSource {
    TaskGrants,
    WorkflowSteps,
    WorkflowTimers,
    TaskBoard,
    TaskDispatchQueue,
    DependencyWaiters,
    WorkerDispatches,
    ConversationInFlight,
    WorkerResultRelays,
    OwnerNotificationQueue,
    SpendAlerts,
    StandingRulePendingActions,
    StandingRuleReservations,
    OwnerReviews,
    ActionRequests,
    ProposedArtifacts,
    EffectFences,
}

impl OutstandingWorkSource {
    pub(crate) const ALL: [Self; 17] = [
        Self::TaskGrants,
        Self::WorkflowSteps,
        Self::WorkflowTimers,
        Self::TaskBoard,
        Self::TaskDispatchQueue,
        Self::DependencyWaiters,
        Self::WorkerDispatches,
        Self::ConversationInFlight,
        Self::WorkerResultRelays,
        Self::OwnerNotificationQueue,
        Self::SpendAlerts,
        Self::StandingRulePendingActions,
        Self::StandingRuleReservations,
        Self::OwnerReviews,
        Self::ActionRequests,
        Self::ProposedArtifacts,
        Self::EffectFences,
    ];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::TaskGrants => "task_grants",
            Self::WorkflowSteps => "workflow_step_registry",
            Self::WorkflowTimers => "workflow_timers",
            Self::TaskBoard => "task_board",
            Self::TaskDispatchQueue => "dispatch_state",
            Self::DependencyWaiters => "task_dependency_waiters",
            Self::WorkerDispatches => "worker_dispatch",
            Self::ConversationInFlight => "conversation_in_flight",
            Self::WorkerResultRelays => "worker_result_relays",
            Self::OwnerNotificationQueue => "notify_dead_letters",
            Self::SpendAlerts => "daily_spend.alert_state",
            Self::StandingRulePendingActions => "standing_rule_pending_actions",
            Self::StandingRuleReservations => "standing_rule_usage",
            Self::OwnerReviews => "owner_reviews",
            Self::ActionRequests => "action_requests",
            Self::ProposedArtifacts => "proposed_artifacts",
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
        let as_of_nanos =
            i64::try_from(as_of.as_nanosecond()).map_err(|_| StoreError::NumericRange)?;
        let as_of_text = crate::store::sql_timestamp(as_of);
        self.with_deferred_read(|tx| {
            let mut entries = Vec::with_capacity(OutstandingWorkSource::ALL.len());
            for source in OutstandingWorkSource::ALL {
                let (total, outstanding, terminal) =
                    source_counts(tx, source, &as_of_text, as_of_nanos)?;
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
    as_of_text: &str,
    as_of_nanos: i64,
) -> Result<(i64, i64, i64), StoreError> {
    let counts = match source {
        OutstandingWorkSource::TaskGrants => tx.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN expires_at > ?1 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN expires_at <= ?1 THEN 1 ELSE 0 END), 0)
             FROM task_grants",
            [as_of_text],
            count_row,
        )?,
        OutstandingWorkSource::WorkflowSteps => tx.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN completed_seq IS NULL OR completed_seq < 0 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN completed_seq >= 0 THEN 1 ELSE 0 END), 0)
             FROM workflow_step_registry",
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
        OutstandingWorkSource::TaskBoard => tx.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN status IN ('open', 'blocked') THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN status IN ('done', 'cancelled') THEN 1 ELSE 0 END), 0)
             FROM task_board",
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
        OutstandingWorkSource::WorkerDispatches => tx.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN state = 'dispatched' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN state = 'terminal' THEN 1 ELSE 0 END), 0)
             FROM worker_dispatch",
            [],
            count_row,
        )?,
        OutstandingWorkSource::ConversationInFlight => tx.query_row(
            "SELECT COUNT(*), COUNT(*), 0 FROM conversation_in_flight",
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
        OutstandingWorkSource::SpendAlerts => tx.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN alert_state <> 0 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN alert_state = 0 THEN 1 ELSE 0 END), 0)
             FROM daily_spend",
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
        OutstandingWorkSource::OwnerReviews => tx.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN state = 'pending' AND expires_at > ?1 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE
                        WHEN state = 'pending' AND expires_at <= ?1 THEN 1
                        WHEN state IN ('approved', 'rejected', 'narrowed', 'revoked', 'expired') THEN 1
                        ELSE 0 END), 0)
             FROM owner_reviews",
            [as_of_nanos],
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
        OutstandingWorkSource::ProposedArtifacts => tx.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE
                        WHEN state IN ('proposed', 'validated', 'review_required', 'approved') THEN 1
                        ELSE 0 END), 0),
                    COALESCE(SUM(CASE
                        WHEN state IN ('active', 'quarantined', 'retired') THEN 1
                        ELSE 0 END), 0)
             FROM proposed_artifacts",
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
