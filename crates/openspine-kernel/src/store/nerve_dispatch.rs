//! Kernel-owned nerve advisee-limit seeding and production dispatch
//! (AD-130/AD-132/AD-034). Advisee limits derive from the loaded
//! `AgentManifest` registry — never from a caller-supplied upsert — and the
//! dispatcher periodically replays registered nerves through the first real
//! handler: the AD-034 screener.
//!
//! Detection of manipulation markers happens at *ingestion* (`screen_text`,
//! called from `pipeline/driver.rs`'s owner-control lane, where the
//! authorized plaintext is available) and emits a structured, scoped
//! `manipulation_signal.detected` carrying only the detected marker. This
//! ingestion screen runs unconditionally (it is pure detection + a ledger
//! append — no authority is required to observe your own inbound plaintext
//! and record a structured finding about it). The dispatcher handler replays
//! that signal and interjects through the budgeted admission path — but ONLY
//! for a nerve that is actually registered; per the governing spec ("Registered
//! nerves replay through typed handlers"), the kernel's job here is a correct
//! replay+handler substrate, not registering a specific declaration itself.
//! Raw payload refs are never resolved by a replaying nerve, and the screener
//! tag's `tagged_aggregate` is derived from the signal event's own aggregate
//! (`owner_control`, set by the kernel at ingestion) — never from
//! caller-asserted JSON (AD-051/AD-034).

use super::Store;
use openspine_schemas::agent::AgentManifest;
use openspine_schemas::artifact::{ArtifactRef, Lifecycle};
use openspine_schemas::audit::AuditEvent;
use openspine_schemas::nerve::{
    InterjectionProvenance, ModelTier, NerveDeclaration, NerveError, NerveScope, NerveType,
    ScreenerTag, Severity,
};
use rusqlite::{params, TransactionBehavior};
use ulid::Ulid;

/// Deterministic, documented heuristic markers for a first real AD-034
/// screener. This is intentionally not an ML classifier — "detection is
/// intelligence, containment is the guarantee" (AD-034): the kernel's job
/// is the budgeted, structured containment lane, not the classifier. A
/// more capable detector can replace this list without changing the
/// substrate.
const MANIPULATION_MARKERS: &[&str] = &[
    "ignore previous instructions",
    "ignore all previous instructions",
    "disregard your instructions",
    "disregard all prior instructions",
    "system prompt override",
    "you are now in developer mode",
];

/// The structured audit kind the ingestion screen emits and a screener nerve
/// consumes. It carries only the detected marker — never plaintext, and never
/// a caller-asserted aggregate — so a replaying nerve never resolves an
/// artifact ref. `manipulation_signal` is the matching data class a nerve
/// declaration's scope must grant (e.g. `main_assistant_agent`'s manifest,
/// once a governed manifest change adds it) and `owner_control` is the fixed
/// aggregate the signal is bound to.
pub(crate) const SCREENER_SIGNAL_KIND: &str = "manipulation_signal.detected";
const SCREENER_AGGREGATE: &str = "owner_control";

/// Screen authorized plaintext for a known manipulation marker. Called at
/// ingestion (AD-034), where the raw event text is available; returns the
/// first matching marker, if any.
pub(crate) fn screen_text(text: &str) -> Option<&'static str> {
    let lowered = text.to_lowercase();
    MANIPULATION_MARKERS
        .iter()
        .copied()
        .find(|marker| lowered.contains(*marker))
}

impl Store {
    /// Seed kernel-owned advisee limits from the loaded, active agent
    /// manifest registry. This is a full snapshot replace, not a merge: any
    /// advisee absent from the current registry loses its limits row, so a
    /// retired or unlisted agent falls back to the conservative default of
    /// "no registrable nerve" rather than retaining stale authority across
    /// restarts. Denied classes are subtracted from the allowed set (using
    /// the same exact-or-dot-child boundary the authorization check uses)
    /// before becoming a nerve scope ceiling. The kernel does not yet track
    /// a per-agent model tier, so `ModelTier::Cheap` is the conservative
    /// default until manifests carry one (candidate D-0XX).
    pub(crate) fn seed_advisee_limits_from_manifests<'a>(
        &self,
        manifests: impl IntoIterator<Item = &'a AgentManifest>,
    ) -> Result<(), NerveError> {
        let derived: Vec<(String, NerveScope, ModelTier)> = manifests
            .into_iter()
            .filter(|manifest| manifest.lifecycle_state == Lifecycle::Active)
            .map(|manifest| {
                let denied: Vec<String> = manifest.memory_scope.denied_classes.clone();
                let scope = NerveScope {
                    data_classes: manifest
                        .memory_scope
                        .allowed_classes
                        .iter()
                        .filter(|class| {
                            // `NerveScope` cannot express "email except secret",
                            // so a denied class closes the whole overlapping
                            // branch. Drop an allowed class when it overlaps a
                            // denied class in EITHER direction: the allowed is
                            // the denied class itself, the allowed is a
                            // dot-child of a denied class (`email.secret`
                            // denied ⇒ drop `email.x`), or the allowed is an
                            // ancestor of a denied class (`email` allowed but
                            // `email.secret` denied ⇒ drop `email`, otherwise a
                            // nerve could still register for `email.secret.*`
                            // via `filter_within_scope`). Unrelated prefixes
                            // (`emailx`) are unaffected. This matches the
                            // authorization boundary conservatively.
                            !denied.iter().any(|d| {
                                class.as_str() == d.as_str()
                                    || class
                                        .as_str()
                                        .strip_prefix(d.as_str())
                                        .is_some_and(|rest| rest.starts_with('.'))
                                    || d.as_str()
                                        .strip_prefix(class.as_str())
                                        .is_some_and(|rest| rest.starts_with('.'))
                            })
                        })
                        .cloned()
                        .collect(),
                    data_scopes: manifest.memory_scope.allowed_scopes.clone(),
                };
                (manifest.id.clone(), scope, ModelTier::Cheap)
            })
            .collect();

        let mut conn = self.conn.lock();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|err| NerveError::Storage(err.to_string()))?;
        if derived.is_empty() {
            tx.execute("DELETE FROM nerve_advisee_limits", [])
                .map_err(|err| NerveError::Storage(err.to_string()))?;
        } else {
            let placeholders = derived.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            let sql = format!(
                "DELETE FROM nerve_advisee_limits WHERE advisee_id NOT IN ({placeholders})"
            );
            let ids: Vec<&str> = derived.iter().map(|(id, _, _)| id.as_str()).collect();
            let bound: Vec<&dyn rusqlite::ToSql> =
                ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
            tx.execute(&sql, bound.as_slice())
                .map_err(|err| NerveError::Storage(err.to_string()))?;
        }
        for (advisee_id, scope, tier) in &derived {
            let scope_json =
                serde_json::to_string(scope).map_err(|err| NerveError::Storage(err.to_string()))?;
            let tier_json =
                serde_json::to_string(tier).map_err(|err| NerveError::Storage(err.to_string()))?;
            tx.execute(
                "INSERT INTO nerve_advisee_limits (advisee_id, scope_json, max_tier)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(advisee_id) DO UPDATE SET scope_json = excluded.scope_json,
                                                       max_tier = excluded.max_tier",
                params![advisee_id, scope_json, tier_json],
            )
            .map_err(|err| NerveError::Storage(err.to_string()))?;
        }
        tx.commit()
            .map_err(|err| NerveError::Storage(err.to_string()))?;
        Ok(())
    }

    /// Emit a structured, scoped `manipulation_signal.detected` for a
    /// detected manipulation `marker`, with its aggregate bound to
    /// `aggregate` (supplied by the caller at ingestion). Carries only the
    /// marker — never plaintext and never a caller-asserted aggregate — so a
    /// replaying nerve never resolves an artifact ref (AD-051 / AD-034).
    /// Test-only: production ingestion uses the atomic
    /// `append_event_received_with_screen`.
    #[allow(dead_code)]
    pub(crate) fn append_screener_signal(
        &self,
        marker: &str,
        aggregate: &str,
    ) -> Result<AuditEvent, super::StoreError> {
        let payload = format!("{{\"marker\":\"{marker}\"}}");
        let mut conn = self.conn.lock();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(super::StoreError::Sqlite)?;
        let event = Store::append_audit_conn_with_options(
            &tx,
            SCREENER_SIGNAL_KIND,
            None,
            None,
            None,
            None,
            &[],
            &[],
            Some(aggregate),
            Some(&payload),
        )?;
        tx.commit().map_err(super::StoreError::Sqlite)?;
        Ok(event)
    }

    /// Atomically append the `event.received` ledger row for an inbound
    /// OWNER-CONTROL event AND, if `screen_text` detects a manipulation
    /// marker in the authorized plaintext (`text`), the structured
    /// `manipulation_signal.detected` in the same transaction, bound to the
    /// fixed `owner_control` aggregate. Both rows commit together or not at
    /// all, so a crash between them cannot leave a manipulation-suspect
    /// inbound event permanently unscreened (AD-034: no fail-open window).
    /// Callers MUST restrict this to the owner-control lane: the fixed
    /// `owner_control` aggregate binding would misattribute another lane's
    /// content (e.g. email-preview) to owner-control authority it does not
    /// have. This runs regardless of whether any nerve is registered to
    /// consume the signal — screening your own inbound plaintext and
    /// recording a structured finding needs no nerve authority; only
    /// *interjecting* on it (in `screener_handler`) is budget/scope-gated.
    pub(crate) fn append_event_received_with_screen(
        &self,
        raw_ref: &ArtifactRef,
        text: &str,
    ) -> Result<(AuditEvent, Option<AuditEvent>), super::StoreError> {
        let mut conn = self.conn.lock();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(super::StoreError::Sqlite)?;
        let received = Store::append_audit_conn_with_options(
            &tx,
            "event.received",
            None,
            None,
            None,
            None,
            &[],
            std::slice::from_ref(raw_ref),
            None,
            None,
        )?;
        let signal = if let Some(marker) = screen_text(text) {
            let payload = format!("{{\"marker\":\"{marker}\"}}");
            Some(Store::append_audit_conn_with_options(
                &tx,
                SCREENER_SIGNAL_KIND,
                None,
                None,
                None,
                None,
                &[],
                &[],
                Some(SCREENER_AGGREGATE),
                Some(&payload),
            )?)
        } else {
            None
        };
        tx.commit().map_err(super::StoreError::Sqlite)?;
        Ok((received, signal))
    }
    pub(crate) fn revoke_nerve_registration(&self, nerve_id: Ulid) -> Result<(), NerveError> {
        let mut conn = self.conn.lock();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|err| NerveError::Storage(err.to_string()))?;
        let id = nerve_id.to_string();
        for table in ["nerve_interjection_budgets", "nerve_decay"] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE nerve_id = ?1"),
                params![id],
            )
            .map_err(|err| NerveError::Storage(err.to_string()))?;
        }
        tx.execute(
            "DELETE FROM nerve_interjection_deliveries WHERE interjection_id IN
             (SELECT interjection_id FROM nerve_issuances WHERE nerve_id = ?1)",
            params![id],
        )
        .map_err(|err| NerveError::Storage(err.to_string()))?;
        tx.execute(
            "DELETE FROM nerve_reactions WHERE interjection_id IN
             (SELECT interjection_id FROM nerve_issuances WHERE nerve_id = ?1)",
            params![id],
        )
        .map_err(|err| NerveError::Storage(err.to_string()))?;
        tx.execute(
            "DELETE FROM nerve_issuances WHERE nerve_id = ?1",
            params![id],
        )
        .map_err(|err| NerveError::Storage(err.to_string()))?;
        tx.execute(
            "DELETE FROM consumer_checkpoints WHERE consumer_id = ?1",
            params![format!("nerve:{nerve_id}")],
        )
        .map_err(|err| NerveError::Storage(err.to_string()))?;
        tx.execute(
            "DELETE FROM nerve_registrations WHERE nerve_id = ?1",
            params![id],
        )
        .map_err(|err| NerveError::Storage(err.to_string()))?;
        tx.commit()
            .map_err(|err| NerveError::Storage(err.to_string()))
    }

    pub(crate) fn pending_nerve_deliveries(&self) -> Result<Vec<(String, String)>, NerveError> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare("SELECT interjection_id, class_digest FROM nerve_interjection_deliveries")
            .map_err(|err| NerveError::Storage(err.to_string()))?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|err| NerveError::Storage(err.to_string()))?;
        rows.map(|row| row.map_err(|err| NerveError::Storage(err.to_string())))
            .collect()
    }

    pub(crate) fn ack_nerve_delivery(&self, interjection_id: &str) -> Result<(), NerveError> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM nerve_interjection_deliveries WHERE interjection_id = ?1",
            params![interjection_id],
        )
        .map_err(|err| NerveError::Storage(err.to_string()))?;
        Ok(())
    }
}

/// Kernel-owned nerve dispatcher: periodically replays every registered
/// nerve over its exact persisted filter through the screener handler.
/// Spawned from `main()`'s startup `tokio::select!`, mirroring
/// `workflow::run_timer_driver`'s loop/sleep shape. A handler error is
/// logged, never propagated — one bad tick must not take the kernel down.
/// Registering a specific screener declaration is deliberately NOT this
/// dispatcher's job (see module docs) — it correctly replays whatever is
/// registered, today or after a future explicit registration.
pub(crate) async fn run_nerve_dispatcher(store: &Store, poll_interval: std::time::Duration) -> ! {
    loop {
        if let Err(err) = store.replay_registered_nerves_with(
            |decl| decl.nerve_type == NerveType::Screener,
            |decl, event| screener_handler(store, decl, event),
        ) {
            tracing::error!("nerve dispatcher: {err}");
        }
        tokio::time::sleep(poll_interval).await;
    }
}

/// AD-034 screener handler: consumes the scoped `manipulation_signal.detected`
/// events emitted at ingestion and interjects through the budgeted,
/// event-bound admission path. It never resolves a payload ref — the
/// plaintext was already screened at ingestion and only the structured marker
/// reached the ledger (AD-051: no scope-leak surface). The screener tag's
/// `tagged_aggregate` is derived from the signal event's own aggregate
/// (kernel-set at ingestion), never from JSON. Non-signal events and
/// non-matching signals are a no-op; a malformed signal is surfaced (not
/// silently skipped) so it is not checkpointed unexamined. An
/// exhausted/retired/below-threshold admission is a benign no-op, not a
/// dispatcher failure.
fn screener_handler(
    store: &Store,
    declaration: &NerveDeclaration,
    event: &AuditEvent,
) -> Result<(), String> {
    if event.kind.as_str() != SCREENER_SIGNAL_KIND {
        return Ok(());
    }
    #[derive(serde::Deserialize)]
    struct Signal {
        marker: String,
    }
    let signal: Signal = serde_json::from_str(event.payload_json.as_deref().unwrap_or(""))
        .map_err(|err| format!("malformed screener signal: {err}"))?;
    // Defense-in-depth: only known markers are actionable. A signal carrying
    // an unknown marker is a no-op (the kernel's own ingestion screen only
    // ever emits known markers; this keeps the handler self-contained).
    if !MANIPULATION_MARKERS.contains(&signal.marker.as_str()) {
        return Ok(());
    }
    let provenance = InterjectionProvenance {
        pattern: format!("marker: {}", signal.marker),
        sources: vec![format!("audit:{}", event.id)],
    };
    // AD-034: derive the tagged aggregate from the signal event's own
    // aggregate (set by the kernel at ingestion), not from caller-asserted
    // JSON.
    let tag = ScreenerTag {
        manipulation_class: signal.marker.clone(),
        tagged_aggregate: event.aggregate_id.clone(),
    };
    match store.admit_interjection_for_event(
        declaration.id,
        &signal.marker,
        Severity::Warn,
        0.9,
        provenance,
        true,
        None,
        Some(tag),
        Some(event.aggregate_id.as_str()),
    ) {
        Ok(_) => Ok(()),
        Err(
            NerveError::BudgetExhausted | NerveError::ClassRetired | NerveError::ThresholdNotMet,
        ) => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}

#[cfg(test)]
#[path = "nerve_dispatch_tests.rs"]
mod tests;
