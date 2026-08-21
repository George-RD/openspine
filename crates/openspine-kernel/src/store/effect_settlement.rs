//! Two-phase settlement seam for non-rollbackable external effects
//! (spec #208 D-003).
//!
//! An external effect (e.g. a Gmail draft write) cannot be undone once the
//! provider accepts it, so its durable evidence is written in two phases
//! around the connector call — which the store never performs itself:
//!
//! 1. [`Store::begin_effect`] claims a pending-write fence and audits the claim
//!    in one `Immediate` transaction. The caller then performs the external
//!    effect.
//! 2. [`Store::settle_effect`] writes the settlement transition selected by the
//!    caller-supplied [`EffectDisposition`] — finalize / cancel / retain — each
//!    paired with its audit row in one `Immediate` transaction.
//!
//! This ticket (#216) defines the seam SHAPE and routes it on the typed
//! [`EffectDisposition`] delivered by #238. It performs NO connector-outcome
//! classification: the caller classifies the outcome into a disposition (Effect
//! Truth epic #198 owns that truth) and hands it here. The store owns the fence
//! and settlement writes; it never calls a connector.

use super::audited_effect::AuditDescriptor;
use super::{Store, StoreError};
use crate::api::effect_executors::EffectDisposition;
use ulid::Ulid;

/// Identity of one claimed pending-write fence, returned by
/// [`Store::begin_effect`] and consumed by [`Store::settle_effect`]. Wraps the
/// `pending_draft_writes` row id so the settlement phase resolves exactly the
/// row the begin phase claimed.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EffectFence {
    pending_id: Ulid,
}

/// Fence-identity inputs for [`Store::begin_effect`], mirroring
/// `claim_pending_draft_write`. Carried as plain data so the caller supplies no
/// raw `Connection`.
pub(crate) struct PendingWriteFence<'a> {
    /// Fresh id for the fence row this begin attempts to claim.
    pub id: Ulid,
    /// Task grant the effect is charged against.
    pub grant_id: Ulid,
    /// The `action_requests` row whose durable payload reconstructs the write.
    pub action_request_id: Ulid,
    /// Thread the draft belongs to.
    pub thread_id: &'a str,
    /// Stable identity of the protected request (see `draft_request_fingerprint`).
    pub request_fingerprint: &'a str,
}

/// Outcome of [`Store::begin_effect`].
pub(crate) enum BeginEffect {
    /// The fence row and its audit row committed atomically; the caller owns
    /// the external effect and must later call [`Store::settle_effect`] with
    /// this handle.
    Fenced(EffectFence),
    /// A concurrent begin already holds an unresolved fence for this exact
    /// request; nothing was written (no fence row, no audit row), matching the
    /// `claim_pending_draft_write` lost-claim contract. The caller performs no
    /// external effect and settles nothing.
    AlreadyFenced,
}

impl Store {
    /// Open the first phase of a non-rollbackable external effect: claim the
    /// pending-write fence and audit the claim atomically (spec #208 D-003).
    ///
    /// The fence claim and its audit row commit in one `Immediate` transaction
    /// so a concurrent begin for the same `request_fingerprint` cannot also
    /// reach the provider (the D-050 TOCTOU-closure discipline). On a lost
    /// claim the method writes nothing and returns
    /// [`BeginEffect::AlreadyFenced`], matching `claim_pending_draft_write`'s
    /// return contract.
    ///
    /// Built on the same primitives as [`Store::with_audited_effect`]
    /// (`with_immediate_tx` + `append_audit_conn`) rather than that combinator
    /// directly, because the audit row must NOT be appended on the lost-claim
    /// no-op — `with_audited_effect` always appends after its effect closure.
    pub(crate) fn begin_effect(
        &self,
        fence: PendingWriteFence<'_>,
        audit: AuditDescriptor,
    ) -> Result<BeginEffect, StoreError> {
        self.with_immediate_tx(|tx| {
            if !Self::claim_pending_draft_write_conn(
                tx,
                fence.id,
                fence.grant_id,
                fence.action_request_id,
                fence.thread_id,
                fence.request_fingerprint,
            )? {
                return Ok(BeginEffect::AlreadyFenced);
            }
            Self::append_audit_conn(
                tx,
                audit.kind.as_str(),
                audit.action.as_ref(),
                audit.decision.as_ref(),
                audit.reason.as_deref(),
                audit.task_grant_id,
                &audit.target_refs,
                &audit.payload_refs,
            )?;
            Ok(BeginEffect::Fenced(EffectFence {
                pending_id: fence.id,
            }))
        })
    }

    /// Settle a fenced external effect: write the settlement transition chosen
    /// by `disposition` and its audit row atomically (spec #208 D-003).
    ///
    /// Routes on the typed [`EffectDisposition`] delivered by #238 — the same
    /// finalize / cancel / retain authority as `settle_reservations`:
    /// - [`EffectDisposition::ConfirmedSuccess`] finalizes: resolve the fence.
    /// - [`EffectDisposition::ConfirmedFailure`] and
    ///   [`EffectDisposition::NotAttempted`] cancel: resolve the fence (no
    ///   effect took hold, so nothing is retained).
    /// - [`EffectDisposition::DeliveryUnknown`] retains + fences: leave the
    ///   fence row `pending` so the possibly-landed write is never resent
    ///   without operator reconciliation (the Unattended workhorse's
    ///   no-duplicate-send rule).
    ///
    /// The audit metadata (kind + references) per transition is supplied by the
    /// caller; this method does NOT decide which audit kind belongs to which
    /// real connector outcome — that classification is Effect Truth #198's.
    pub(crate) fn settle_effect(
        &self,
        fence: EffectFence,
        disposition: EffectDisposition,
        audit: AuditDescriptor,
    ) -> Result<(), StoreError> {
        self.with_audited_effect(audit, |tx| {
            match disposition {
                EffectDisposition::ConfirmedSuccess
                | EffectDisposition::ConfirmedFailure
                | EffectDisposition::NotAttempted => {
                    Self::resolve_pending_draft_write_conn(tx, fence.pending_id)?;
                }
                // Retain + fence: leave the row `pending`. No fence write.
                EffectDisposition::DeliveryUnknown => {}
            }
            Ok(())
        })
    }
}

#[cfg(test)]
#[path = "effect_settlement_tests.rs"]
mod tests;
