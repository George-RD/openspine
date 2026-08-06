//! Restart-bound overlay export and restore orchestration.
//!
//! Controller lock is acquired before any store opens. Pending signed
//! operations are processed under that lifetime lock; startup-owned audit
//! persistence happens between `begin_finalization` and `complete_finalization`.

mod bundle;
mod control;
mod install;
mod operation;
mod types;

pub(crate) use control::ControlError;
pub(crate) use operation::{acquire, OverlayOperations};
pub(crate) use types::OverlayOperationError;
pub(crate) use types::{
    CompletionMetadata, FinalizationOutcome, OverlayOperationKind, PendingFinalization,
};

/// Whether an acquire failed because another process holds the lifetime lock.
///
/// Matched on the variant here rather than downcast at the call site:
/// `OverlayOperationError::Control` is `#[error(transparent)]`, so `source()`
/// forwards past the `ControlError` and a chain walk never sees it.
pub(crate) fn is_already_locked(error: &OverlayOperationError) -> bool {
    matches!(
        error,
        OverlayOperationError::Control(ControlError::AlreadyLocked(_))
    )
}

#[cfg(test)]
mod operation_tests;
