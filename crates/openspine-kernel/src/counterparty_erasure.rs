//!
//! Counterparty crypto-erasure (AD-140, resolves OQ-7).
//!
//! `erase_counterparty` is the single entry point that turns "delete a
//! counterparty" into the two AD-140 effects:
//!
//! 1. **Derived artifacts are invalidated via their provenance links**
//!    (D-077). Every `learned_artifacts` row whose `Provenance::ProducedBy`
//!    records `source_scope == counterparty_id` is flipped to
//!    `CompatibilityStatus::Erased` — a terminal state, not a reconfirmation
//!    prompt, because the source exchange is now undecryptable. Matching on
//!    the provenance edge's OWN recorded scope (not a blob-header or
//!    path-existence lookup keyed by digest) is deliberate: two
//!    counterparties can independently store identical plaintext, which
//!    content-addresses to the SAME `source_exchange` digest, so any
//!    digest-keyed resolution cannot tell which counterparty actually
//!    produced a given derived artifact. `source_scope` is recorded once, at
//!    production time, precisely to make this resolution exact.
//! 2. **Payloads become unrecoverable** — the per-counterparty key is deleted
//!    (`CounterpartyKeyRing::erase`). Because no plaintext key is ever cached
//!    in memory across calls, the deletion is final the instant the file is
//!    gone. Per-counterparty blob paths (`ArtifactStore::get_scoped`) mean
//!    this erasure never collaterally destroys another counterparty's copy
//!    of identical plaintext, and never leaves the erased counterparty's copy
//!    recoverable through another counterparty's still-live key.
//!
//! The order is deliberate (audit-before-effect): the derived-artifact
//! invalidation transaction (with its `counterparty.erased` audit row)
//! commits *before* the irreversible key deletion, and the audit row carries
//! only digest/ULID references (D-012). The audit hash chain is only ever
//! appended to, so chain verification keeps passing after an erase.

use ulid::Ulid;

use crate::artifact_store::{ArtifactStore, ArtifactStoreError};
use crate::store::{Store, StoreError};

/// The outcome of a crypto-erase.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // see `erase_counterparty` note: out-of-scope production caller
pub struct ErasureReport {
    /// Number of derived learned artifacts flipped to `Erased`.
    pub derived_artifacts_invalidated: usize,
    /// Exact identities of learned artifacts invalidated by this erasure pass.
    pub invalidated_identities: Vec<crate::store::learned_artifacts::LearnedArtifactIdentity>,
    /// `true` if the counterparty's payload key existed and was deleted.
    pub key_deleted: bool,
}

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)] // see `erase_counterparty` note: out-of-scope production caller
pub enum CounterpartyEraseError {
    #[error("store error during counterparty erasure: {0}")]
    Store(#[from] StoreError),
    #[error("artifact store error during counterparty erasure: {0}")]
    Artifact(#[from] ArtifactStoreError),
}
// Erase `counterparty_id`: invalidate every learned artifact derived from
// its payloads (via provenance links) and crypto-delete its payload key.
//
// Exercised by the `counterparty_erasure` integration tests; the
// owner-facing delete-counterparty *command* (which would call this with a
// real counterparty id) is out of scope for this change (see
// IMPLEMENTATION-NOTES) and intentionally not wired into the gate/UI
// here, so this entry point is allow-ed rather than given a production
// caller in this diff.
#[allow(dead_code)]
pub fn erase_counterparty(
    store: &Store,
    artifacts: &ArtifactStore,
    counterparty_id: Ulid,
) -> Result<ErasureReport, CounterpartyEraseError> {
    if counterparty_id == crate::counterparty_keys::SYSTEM_SCOPE {
        return Err(CounterpartyEraseError::Store(StoreError::LearnedArtifact(
            "SYSTEM_SCOPE is reserved and cannot be erased".into(),
        )));
    }
    let erasure = store.mark_learned_artifacts_erased(counterparty_id, artifacts)?;

    Ok(ErasureReport {
        // Count only rows this pass newly flipped; the identity vector still
        // includes every matching terminal row so cleanup retries remain exact.
        derived_artifacts_invalidated: erasure.newly_invalidated,
        invalidated_identities: erasure.invalidated,
        key_deleted: erasure.key_deleted,
    })
}

#[cfg(test)]
#[path = "counterparty_erasure_tests.rs"]
mod tests;
