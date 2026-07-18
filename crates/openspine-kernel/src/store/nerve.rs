use super::Store;
use openspine_schemas::nerve::{
    AdvisorObjection, InterjectionProvenance, ModelTier, NerveDeclaration, NerveError,
    NerveInterjection, NerveMeasure, NerveScope, NerveType, ScreenerTag,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use ulid::Ulid;

pub(super) fn ensure_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS nerve_registrations (
            nerve_id TEXT PRIMARY KEY,
            advisee_id TEXT NOT NULL,
            declaration_json TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS nerve_advisee_limits (
            advisee_id TEXT PRIMARY KEY,
            scope_json TEXT NOT NULL,
            max_tier TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS nerve_interjection_budgets (
            nerve_id TEXT NOT NULL,
            window_kind TEXT NOT NULL,
            window_started_ns INTEGER NOT NULL,
            used INTEGER NOT NULL DEFAULT 0,
            max INTEGER NOT NULL,
            PRIMARY KEY (nerve_id, window_kind),
            FOREIGN KEY (nerve_id) REFERENCES nerve_registrations(nerve_id)
        );
        CREATE TABLE IF NOT EXISTS nerve_decay (
            nerve_id TEXT NOT NULL,
            class TEXT NOT NULL,
            ignored_count INTEGER NOT NULL DEFAULT 0,
            retired INTEGER NOT NULL DEFAULT 0,
            engaged_count INTEGER NOT NULL DEFAULT 0,
            annoyed_count INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (nerve_id, class),
            FOREIGN KEY (nerve_id) REFERENCES nerve_registrations(nerve_id)
        );
        CREATE TABLE IF NOT EXISTS nerve_issuances (
            interjection_id TEXT PRIMARY KEY,
            nerve_id TEXT NOT NULL,
            advisee_id TEXT NOT NULL,
            nerve_type TEXT NOT NULL,
            class_digest TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS nerve_reactions (
            interjection_id TEXT PRIMARY KEY,
            reaction TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS nerve_interjection_deliveries (
            interjection_id TEXT PRIMARY KEY,
            class_digest TEXT NOT NULL,
            gate_visible INTEGER NOT NULL
        );",
    )
}
fn storage<T>(result: Result<T, rusqlite::Error>) -> Result<T, NerveError> {
    result.map_err(|err| NerveError::Storage(err.to_string()))
}

fn class_digest(class: &str) -> String {
    openspine_schemas::digest::digest_of_bytes(class.as_bytes()).to_string()
}
fn filter_within_scope(declaration: &NerveDeclaration) -> bool {
    let scope = &declaration.scope;
    if scope.data_classes.is_empty() && scope.data_scopes.is_empty() {
        return false;
    }
    let Some(kinds) = &declaration.subscription_filter.kinds else {
        return false;
    };
    let Some(aggregate) = &declaration.subscription_filter.aggregate_id else {
        return false;
    };
    if !scope.data_scopes.contains(aggregate) {
        return false;
    }
    kinds.iter().all(|kind| {
        scope
            .data_classes
            .iter()
            .any(|class| kind.as_str() == class || kind.as_str().starts_with(&format!("{class}.")))
    })
}

#[allow(dead_code)]
impl Store {
    /// Store the kernel-owned advisee limits used by later registration. The
    /// registration + initial budget row are one transaction, so a failed
    /// registration leaves no partial state.
    pub(crate) fn register_advisee_limits(
        &self,
        advisee_id: &str,
        scope: &NerveScope,
        max_tier: ModelTier,
    ) -> Result<(), NerveError> {
        let conn = self.conn.lock();
        let scope_json =
            serde_json::to_string(scope).map_err(|err| NerveError::Storage(err.to_string()))?;
        storage(conn.execute(
            "INSERT INTO nerve_advisee_limits (advisee_id, scope_json, max_tier)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(advisee_id) DO UPDATE SET scope_json = excluded.scope_json,
                                                   max_tier = excluded.max_tier",
            params![
                advisee_id,
                scope_json,
                serde_json::to_string(&max_tier)
                    .map_err(|err| NerveError::Storage(err.to_string()))?
            ],
        ))?;
        Ok(())
    }
    /// Register using limits resolved from the kernel-owned advisee table.
    pub(crate) fn register_nerve(&self, declaration: &NerveDeclaration) -> Result<(), NerveError> {
        if declaration.schema_version != 1 || declaration.subscription_filter.schema_version != 1 {
            return Err(NerveError::Storage(
                "unsupported nerve/filter schema version".to_string(),
            ));
        }
        let expected_measure = match declaration.nerve_type {
            NerveType::Advisor => NerveMeasure::Legibility,
            NerveType::Injector => NerveMeasure::SkillMatch,
            NerveType::Screener => NerveMeasure::ManipulationTag,
            NerveType::Miner => NerveMeasure::SystemicPattern,
            NerveType::MetaCognition => NerveMeasure::SecondOrderHealth,
        };
        if declaration.measure != expected_measure {
            return Err(NerveError::InvalidPayload);
        }
        if declaration.budget.suggestions_max == 0
            || declaration.budget.window_kind.is_empty()
            || declaration.budget.window_seconds == 0
        {
            return Err(NerveError::Storage("invalid nerve budget".to_string()));
        }
        declaration.speak_threshold.validate()?;

        let mut conn = self.conn.lock();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|err| NerveError::Storage(err.to_string()))?;
        let (scope_json, tier_json): (String, String) = storage(tx.query_row(
            "SELECT scope_json, max_tier FROM nerve_advisee_limits WHERE advisee_id = ?1",
            params![declaration.advisee_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ))?;
        let advisee_scope: NerveScope = serde_json::from_str(&scope_json)
            .map_err(|err| NerveError::Storage(err.to_string()))?;
        let advisee_max_tier: ModelTier =
            serde_json::from_str(&tier_json).map_err(|err| NerveError::Storage(err.to_string()))?;
        if !declaration.is_scope_within(&advisee_scope)
            || !declaration.is_tier_within(advisee_max_tier)
        {
            return Err(if !declaration.is_scope_within(&advisee_scope) {
                NerveError::ScopeExceedsAdvisee
            } else {
                NerveError::TierExceedsAdvisee
            });
        }
        if !filter_within_scope(declaration) {
            return Err(NerveError::ScopeExceedsAdvisee);
        }
        let existing: Option<String> = storage(
            tx.query_row(
                "SELECT nerve_id FROM nerve_registrations WHERE nerve_id = ?1",
                params![declaration.id.to_string()],
                |row| row.get(0),
            )
            .optional(),
        )?;
        if existing.is_some() {
            return Err(NerveError::AlreadyRegistered);
        }
        let json = serde_json::to_string(declaration)
            .map_err(|err| NerveError::Storage(err.to_string()))?;
        let window_started_ns = i64::try_from(jiff::Timestamp::now().as_nanosecond())
            .map_err(|_| NerveError::Storage("timestamp out of SQLite range".to_string()))?;
        tx.execute(
            "INSERT INTO nerve_registrations (nerve_id, advisee_id, declaration_json)
             VALUES (?1, ?2, ?3)",
            params![declaration.id.to_string(), declaration.advisee_id, json],
        )
        .map_err(|err| NerveError::Storage(err.to_string()))?;
        tx.execute(
            "INSERT INTO nerve_interjection_budgets
             (nerve_id, window_kind, window_started_ns, used, max)
             VALUES (?1, ?2, ?3, 0, ?4)",
            params![
                declaration.id.to_string(),
                declaration.budget.window_kind,
                window_started_ns,
                i64::from(declaration.budget.suggestions_max)
            ],
        )
        .map_err(|err| NerveError::Storage(err.to_string()))?;
        let consumer_id = format!("nerve:{}", declaration.id);
        super::event_bus::bind_consumer_conn(&tx, &consumer_id, &declaration.subscription_filter)
            .map_err(|err| NerveError::Storage(err.to_string()))?;
        tx.commit()
            .map_err(|err| NerveError::Storage(err.to_string()))?;
        Ok(())
    }

    /// Load the canonical registered declaration. Callers cannot widen a
    /// budget by passing a locally modified copy to admission.
    pub(crate) fn load_nerve(
        &self,
        nerve_id: Ulid,
    ) -> Result<Option<NerveDeclaration>, NerveError> {
        let conn = self.conn.lock();
        let json: Option<String> = storage(
            conn.query_row(
                "SELECT declaration_json FROM nerve_registrations WHERE nerve_id = ?1",
                params![nerve_id.to_string()],
                |row| row.get(0),
            )
            .optional(),
        )?;
        json.map(|value| {
            serde_json::from_str(&value).map_err(|err| NerveError::Storage(err.to_string()))
        })
        .transpose()
    }

    pub(crate) fn replay_registered_nerves<F>(&self, handler: F) -> Result<(), NerveError>
    where
        F: FnMut(&NerveDeclaration, &openspine_schemas::audit::AuditEvent) -> Result<(), String>,
    {
        self.replay_registered_nerves_with(|_| true, handler)
    }

    pub(crate) fn replay_registered_nerves_with<F, P>(
        &self,
        predicate: P,
        mut handler: F,
    ) -> Result<(), NerveError>
    where
        F: FnMut(&NerveDeclaration, &openspine_schemas::audit::AuditEvent) -> Result<(), String>,
        P: Fn(&NerveDeclaration) -> bool,
    {
        let declarations: Vec<NerveDeclaration> = {
            let conn = self.conn.lock();
            let mut stmt = conn
                .prepare("SELECT declaration_json FROM nerve_registrations")
                .map_err(|err| NerveError::Storage(err.to_string()))?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|err| NerveError::Storage(err.to_string()))?;
            rows.map(|row| {
                let json = row.map_err(|err| NerveError::Storage(err.to_string()))?;
                serde_json::from_str(&json).map_err(|err| NerveError::Storage(err.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?
        };
        for declaration in declarations {
            let limits = {
                let conn = self.conn.lock();
                conn.query_row(
                    "SELECT scope_json, max_tier FROM nerve_advisee_limits WHERE advisee_id = ?1",
                    params![declaration.advisee_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()
                .map_err(|err| NerveError::Storage(err.to_string()))?
            };
            let Some((scope_json, tier_json)) = limits else {
                self.revoke_nerve_registration(declaration.id)?;
                continue;
            };
            let advisee_scope: NerveScope = serde_json::from_str(&scope_json)
                .map_err(|err| NerveError::Storage(err.to_string()))?;
            let advisee_tier: ModelTier = serde_json::from_str(&tier_json)
                .map_err(|err| NerveError::Storage(err.to_string()))?;
            if !declaration.is_scope_within(&advisee_scope)
                || !declaration.is_tier_within(advisee_tier)
                || !filter_within_scope(&declaration)
            {
                self.revoke_nerve_registration(declaration.id)?;
                continue;
            }
            if !predicate(&declaration) {
                continue;
            }
            let consumer_id = format!("nerve:{}", declaration.id);
            let mut consumer = super::event_bus::IdempotentConsumer::with_persisted_checkpoint(
                self,
                consumer_id,
                declaration.subscription_filter.clone(),
            )
            .map_err(|err| NerveError::Storage(err.to_string()))?;
            consumer
                .replay(self, &mut (), |_, event| handler(&declaration, event))
                .map_err(|err| NerveError::Storage(err.to_string()))?;
        }
        Ok(())
    }

    /// Thin wrapper for callers without a triggering event; passes `None`,
    /// so screener tags are rejected (AD-034 aggregate binding).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn admit_interjection(
        &self,
        nerve_id: Ulid,
        class: &str,
        severity: openspine_schemas::nerve::Severity,
        confidence: f64,
        provenance: InterjectionProvenance,
        gate_visible: bool,
        advisor: Option<AdvisorObjection>,
        screener: Option<ScreenerTag>,
    ) -> Result<NerveInterjection, NerveError> {
        self.admit_interjection_for_event(
            nerve_id,
            class,
            severity,
            confidence,
            provenance,
            gate_visible,
            advisor,
            screener,
            None,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn admit_interjection_for_event(
        &self,
        nerve_id: Ulid,
        class: &str,
        severity: openspine_schemas::nerve::Severity,
        confidence: f64,
        provenance: InterjectionProvenance,
        gate_visible: bool,
        advisor: Option<AdvisorObjection>,
        screener: Option<ScreenerTag>,
        triggering_aggregate: Option<&str>,
    ) -> Result<NerveInterjection, NerveError> {
        if class.is_empty()
            || provenance.pattern.is_empty()
            || provenance.sources.is_empty()
            || provenance.sources.iter().any(String::is_empty)
        {
            return Err(NerveError::InvalidProvenance);
        }
        let declaration = self.load_nerve(nerve_id)?.ok_or(NerveError::NotFound)?;
        if let Some(objection) = advisor.as_ref() {
            if objection.concern_class.is_empty()
                || objection.concern_class.chars().count() > 256
                || objection.cited_clause.is_empty()
                || objection.cited_clause.chars().count() > 256
            {
                return Err(NerveError::InvalidPayload);
            }
        }
        if let Some(tag) = screener.as_ref() {
            if tag.manipulation_class.is_empty() || tag.tagged_aggregate.is_empty() {
                return Err(NerveError::InvalidPayload);
            }
            // AD-034: the tag must name the aggregate of the event that
            // actually triggered it, not merely a filter's static
            // constraint (a filter can match multiple aggregates).
            if triggering_aggregate != Some(tag.tagged_aggregate.as_str()) {
                return Err(NerveError::InvalidPayload);
            }
        }
        let payload_class = match declaration.nerve_type {
            NerveType::Advisor => Some(
                advisor
                    .as_ref()
                    .ok_or(NerveError::InvalidPayload)?
                    .concern_class
                    .as_str(),
            ),
            NerveType::Screener => Some(
                screener
                    .as_ref()
                    .ok_or(NerveError::InvalidPayload)?
                    .manipulation_class
                    .as_str(),
            ),
            _ => None,
        };
        if payload_class.is_some_and(|payload| payload.is_empty() || payload != class) {
            return Err(NerveError::InvalidPayload);
        }

        declaration.evaluate_admission(severity, confidence, false)?;
        let mut conn = self.conn.lock();
        let now_ns = i64::try_from(jiff::Timestamp::now().as_nanosecond())
            .map_err(|_| NerveError::Storage("timestamp out of SQLite range".to_string()))?;
        let window_ns =
            i64::try_from(i128::from(declaration.budget.window_seconds) * 1_000_000_000_i128)
                .map_err(|_| {
                    NerveError::Storage("budget window out of SQLite range".to_string())
                })?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|err| NerveError::Storage(err.to_string()))?;
        let changed = tx
            .execute(
                "UPDATE nerve_interjection_budgets
                 SET used = CASE WHEN ?3 - window_started_ns >= ?4 THEN 1 ELSE used + 1 END,
                     window_started_ns = CASE
                         WHEN ?3 - window_started_ns >= ?4 THEN ?3
                         ELSE window_started_ns
                     END
                 WHERE nerve_id = ?1 AND window_kind = ?2 AND max > 0
                   AND NOT EXISTS (
                       SELECT 1 FROM nerve_decay
                       WHERE nerve_id = ?1 AND class = ?5 AND retired = 1
                   )
                   AND (used < max OR ?3 - window_started_ns >= ?4)",
                params![
                    nerve_id.to_string(),
                    declaration.budget.window_kind,
                    now_ns,
                    window_ns,
                    class_digest(class)
                ],
            )
            .map_err(|err| NerveError::Storage(err.to_string()))?;
        if changed != 1 {
            let retired: Option<i64> = tx
                .query_row(
                    "SELECT retired FROM nerve_decay WHERE nerve_id = ?1 AND class = ?2",
                    params![nerve_id.to_string(), class_digest(class)],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|err| NerveError::Storage(err.to_string()))?;
            tx.rollback()
                .map_err(|err| NerveError::Storage(err.to_string()))?;
            return Err(if retired == Some(1) {
                NerveError::ClassRetired
            } else {
                NerveError::BudgetExhausted
            });
        }
        let gate_visible = matches!(
            declaration.nerve_type,
            NerveType::Advisor | NerveType::Screener
        ) || gate_visible;
        let interjection_id = Ulid::new();
        tx.execute(
            "INSERT INTO nerve_issuances
             (interjection_id, nerve_id, advisee_id, nerve_type, class_digest)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                interjection_id.to_string(),
                nerve_id.to_string(),
                declaration.advisee_id.clone(),
                serde_json::to_string(&declaration.nerve_type)
                    .map_err(|err| NerveError::Storage(err.to_string()))?,
                class_digest(class)
            ],
        )
        .map_err(|err| NerveError::Storage(err.to_string()))?;

        if gate_visible {
            tx.execute(
                "INSERT INTO nerve_interjection_deliveries (interjection_id, class_digest, gate_visible)
                 VALUES (?1, ?2, 1)",
                params![interjection_id.to_string(), class_digest(class)],
            )
            .map_err(|err| NerveError::Storage(err.to_string()))?;
        }
        let mut interjection = NerveInterjection {
            id: interjection_id,
            nerve_id: nerve_id.to_string(),
            advisee_id: declaration.advisee_id,
            nerve_type: declaration.nerve_type,
            class: class.to_string(),
            severity,
            provenance,
            gate_visible: matches!(
                declaration.nerve_type,
                NerveType::Advisor | NerveType::Screener
            ) || gate_visible,
            advisor: None,
            screener: None,
        };
        match declaration.nerve_type {
            NerveType::Advisor => interjection.advisor = advisor,
            NerveType::Screener => interjection.screener = screener,
            _ => {}
        }
        tx.commit()
            .map_err(|err| NerveError::Storage(err.to_string()))?;
        Ok(interjection)
    }
}

#[cfg(test)]
#[path = "nerve_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "nerve_concurrency_tests.rs"]
mod concurrency_tests;
