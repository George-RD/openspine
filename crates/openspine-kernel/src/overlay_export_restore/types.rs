//! Public orchestration types for overlay export/restore.

use std::path::PathBuf;

use thiserror::Error;

use super::bundle::BundleError;
use super::control::ControlError;

#[derive(Debug, Error)]
pub(crate) enum OverlayOperationError {
    #[error(transparent)]
    Control(#[from] ControlError),
    #[error("bundle operation failed: {0}")]
    Bundle(String),
    #[error("overlay operation is not recoverable at stage {stage}")]
    UnrecoverableStage { stage: String },
    #[error("restore install state is missing for recovery")]
    MissingInstallState,
    #[error("restore install state is ambiguous")]
    AmbiguousInstallState,
    #[error("filesystem operation failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl From<BundleError> for OverlayOperationError {
    fn from(value: BundleError) -> Self {
        Self::Bundle(value.to_string())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OverlayOperationKind {
    Export,
    Restore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FinalizationOutcome {
    Completed,
    RolledBack,
}

/// Pre-open work result retained until post-bind finalization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PendingFinalization {
    pub(crate) kind: OverlayOperationKind,
    pub(crate) outcome: FinalizationOutcome,
    pub(crate) request_id: String,
    pub(crate) action_id: String,
    pub(crate) owner_principal_id: String,
    pub(crate) grant_id: String,
    pub(crate) bundle_name: String,
    pub(crate) path_digest: String,
    pub(crate) requested_at: String,
}

/// Typed completion metadata for startup-owned audit appends.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompletionMetadata {
    pub(crate) kind: OverlayOperationKind,
    pub(crate) outcome: FinalizationOutcome,
    pub(crate) request_id: String,
    pub(crate) action_id: String,
    pub(crate) owner_principal_id: String,
    pub(crate) grant_id: String,
    pub(crate) bundle_name: String,
    pub(crate) path_digest: String,
    pub(crate) requested_at: String,
    pub(crate) completed_at: String,
}
