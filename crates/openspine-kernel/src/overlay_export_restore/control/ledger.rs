//! Terminal-erasure ledger validation, comparison, and operation transition
//! logic for overlay control.

use super::wire::{
    OperationKind, OperationStage, SignedTerminalLedger, TerminalLedger, FORMAT_VERSION,
};
use super::ControlError;

// ── Operation stage transitions ─────────────────────────────────────────────

pub(super) fn valid_transition(
    kind: OperationKind,
    current: OperationStage,
    next: OperationStage,
) -> bool {
    use OperationStage::*;
    matches!(
        (kind, current, next),
        (OperationKind::Export, Requested, Staged)
            | (OperationKind::Export, Staged, Finalizing)
            | (OperationKind::Restore, Requested, Staged)
            | (OperationKind::Restore, Staged, Installed)
            | (OperationKind::Restore, Installed, Finalizing)
            | (
                OperationKind::Restore,
                Installed | Finalizing,
                RollbackRequested
            )
            | (OperationKind::Restore, RollbackRequested, RolledBack)
    )
}

// ── Counterparty ID validation ──────────────────────────────────────────────

pub(super) fn validate_counterparty_id(value: &str) -> Result<(), ControlError> {
    let ok =
        !value.is_empty() && value.len() <= 128 && value.bytes().all(|b| b.is_ascii_alphanumeric());
    ok.then_some(()).ok_or(ControlError::InvalidCounterpartyId)
}

// ── Ledger validation ───────────────────────────────────────────────────────

pub(super) fn validate_ledger(ledger: &TerminalLedger) -> Result<(), ControlError> {
    if ledger.version != FORMAT_VERSION
        || ledger.continuity_id.is_empty()
        || ledger.continuity_id.len() > 64
        || !ledger
            .continuity_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric())
        || ledger.sequence != ledger.erased_counterparty_ids.len() as u64
        || ledger
            .erased_counterparty_ids
            .iter()
            .any(|id| validate_counterparty_id(id).is_err())
    {
        return Err(ControlError::MalformedState(
            "invalid terminal ledger".into(),
        ));
    }
    Ok(())
}

/// Return the newer of two ledgers. Different continuity identities diverge.
pub(super) fn newer_ledger(
    left: SignedTerminalLedger,
    right: SignedTerminalLedger,
) -> Result<SignedTerminalLedger, ControlError> {
    if left.body.continuity_id != right.body.continuity_id {
        return Err(ControlError::DivergedContinuity);
    }
    if left.body.sequence == right.body.sequence {
        return if left.body.erased_counterparty_ids == right.body.erased_counterparty_ids {
            Ok(left)
        } else {
            Err(ControlError::DivergedContinuity)
        };
    }
    let (newer, older) = if left.body.sequence > right.body.sequence {
        (left, right)
    } else {
        (right, left)
    };
    if newer
        .body
        .erased_counterparty_ids
        .is_superset(&older.body.erased_counterparty_ids)
    {
        Ok(newer)
    } else {
        Err(ControlError::DivergedContinuity)
    }
}
