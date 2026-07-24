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
pub(crate) use types::{
    CompletionMetadata, FinalizationOutcome, OverlayOperationKind, PendingFinalization,
};

#[cfg(test)]
mod operation_tests;
