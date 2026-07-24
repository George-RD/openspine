//!
//! Counterparty crypto-erasure (AD-140, resolves OQ-7).
//!
//! `erase_counterparty` is the single entry point that turns "delete a
//! counterparty" into the AD-140 effects, now sequenced behind the signed
//! monotonic terminal-erasure ledger:
//!
//! 1. **Ledger first.** `OverlayOperations::record_terminal_erasure` durably
//!    records the counterparty id in the authenticated terminal ledger.
//! 2. **In-memory close.** While the per-scope lock is still held, the
//!    generation-local process marks the scope closed without deleting key
//!    material. Later same-process reads/writes fail closed immediately.
//! 3. **Local invalidation.** The learned-artifact transaction, audit row,
//!    runtime revocation, and irreversible key tombstone/deletion run only
//!    after steps 1–2. Failures after the ledger write remain retryable: the
//!    scope stays closed in memory and startup reconciliation re-applies the
//!    same signed ledger ids.
//!
//! Matching derived artifacts by provenance `source_scope` (not digest) is
//! deliberate: two counterparties can store identical plaintext. The audit
//! hash chain only ever appends, so chain verification keeps passing.

use ulid::Ulid;

use crate::artifact_store::{ArtifactStore, ArtifactStoreError};
use crate::overlay_export_restore::{ControlError, OverlayOperations};
use crate::store::{Store, StoreError};

/// The outcome of a crypto-erase.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // exercised by focused tests; owner command is a follow-up
pub struct ErasureReport {
    /// Number of derived learned artifacts flipped to `Erased`.
    pub derived_artifacts_invalidated: usize,
    /// Exact identities of learned artifacts invalidated by this erasure pass.
    pub invalidated_identities: Vec<crate::store::learned_artifacts::LearnedArtifactIdentity>,
    /// `true` if the counterparty's payload key existed and was deleted.
    pub key_deleted: bool,
    /// Terminal ledger sequence after the durable id was recorded.
    pub ledger_sequence: u64,
}

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)] // exercised by focused tests; owner command is a follow-up
pub enum CounterpartyEraseError {
    #[error("store error during counterparty erasure: {0}")]
    Store(#[from] StoreError),
    #[error("artifact store error during counterparty erasure: {0}")]
    Artifact(#[from] ArtifactStoreError),
    #[error("overlay control error during counterparty erasure: {0}")]
    Control(#[from] ControlError),
}

/// Erase `counterparty_id` under the signed terminal ledger.
///
/// Ordering is fail-closed: a ledger-write failure leaves generation-local
/// state untouched; any post-ledger failure keeps the scope closed in memory
/// so startup can retry the same signed id.
#[allow(dead_code)]
pub fn erase_counterparty(
    store: &Store,
    artifacts: &ArtifactStore,
    operations: &OverlayOperations,
    counterparty_id: Ulid,
) -> Result<ErasureReport, CounterpartyEraseError> {
    reject_system_scope(counterparty_id)?;

    // Hold the same lock as scoped writes so no concurrent put can mint a
    // key after the ledger entry is durable but before in-memory closure.
    let ledger_sequence = artifacts.with_scope_lock(counterparty_id, || {
        let ledger = operations.record_terminal_erasure(&counterparty_id.to_string())?;
        // No-delete in-memory closure: block access before generation-local
        // SQLite invalidation or key deletion. Failures after this point are
        // retryable and remain fail-closed in this process.
        artifacts.close_counterparty_scope_in_memory(counterparty_id);
        Ok::<u64, CounterpartyEraseError>(ledger.sequence())
    })?;

    let mut report = finish_local_erasure(store, artifacts, counterparty_id)?;
    report.ledger_sequence = ledger_sequence;
    Ok(report)
}

/// Reapply every authenticated terminal ledger id to the opened generation.
///
/// Each id is closed in memory before the existing erasure transaction,
/// audit, runtime invalidation, and key cleanup. The path is idempotent, so
/// repeated startup reconciliation is safe.
pub(crate) fn reconcile_overlay_terminal_erasures(
    store: &Store,
    artifacts: &ArtifactStore,
    operations: &OverlayOperations,
) -> Result<(), CounterpartyEraseError> {
    let ledger = operations.export_terminal_ledger()?;
    for encoded in ledger.erased_counterparty_ids() {
        let counterparty_id = Ulid::from_string(encoded)
            .map_err(|_| CounterpartyEraseError::Store(StoreError::BadUlid(encoded.clone())))?;
        reject_system_scope(counterparty_id)?;
        artifacts.with_scope_lock(counterparty_id, || {
            artifacts.close_counterparty_scope_in_memory(counterparty_id);
            Ok::<(), CounterpartyEraseError>(())
        })?;
        let _ = finish_local_erasure(store, artifacts, counterparty_id)?;
    }
    Ok(())
}

fn reject_system_scope(counterparty_id: Ulid) -> Result<(), CounterpartyEraseError> {
    if counterparty_id == crate::counterparty_keys::SYSTEM_SCOPE {
        return Err(CounterpartyEraseError::Store(StoreError::LearnedArtifact(
            "SYSTEM_SCOPE is reserved and cannot be erased".into(),
        )));
    }
    Ok(())
}

fn finish_local_erasure(
    store: &Store,
    artifacts: &ArtifactStore,
    counterparty_id: Ulid,
) -> Result<ErasureReport, CounterpartyEraseError> {
    let erasure = store.mark_learned_artifacts_erased(counterparty_id, artifacts)?;
    Ok(ErasureReport {
        derived_artifacts_invalidated: erasure.newly_invalidated,
        invalidated_identities: erasure.invalidated,
        key_deleted: erasure.key_deleted,
        ledger_sequence: 0,
    })
}

#[cfg(test)]
#[path = "counterparty_erasure_tests.rs"]
mod tests;
