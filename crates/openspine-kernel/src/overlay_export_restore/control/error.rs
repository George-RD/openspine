use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ControlError {
    #[error("data root is a symlink: {0}")]
    SymlinkDataRoot(PathBuf),
    #[error("path is not a directory: {0}")]
    NotDirectory(PathBuf),
    #[error("protected control path has an unsafe type: {0}")]
    UnsafeControlPath(PathBuf),
    #[error("data root is already locked: {0}")]
    AlreadyLocked(PathBuf),
    #[error("invalid bundle name")]
    InvalidBundleName,
    #[error("invalid overlay operation action: {0}")]
    InvalidAction(String),
    #[error("an overlay operation is already pending")]
    OperationPending,
    #[error("no overlay operation is pending")]
    NoPendingOperation,
    #[error("pending operation request does not match")]
    RequestMismatch,
    #[error("invalid operation stage transition")]
    InvalidTransition,
    #[error("signed control state authentication failed")]
    AuthenticationFailed,
    #[error("signed control state is malformed: {0}")]
    MalformedState(String),
    #[error("terminal-erasure continuity is missing")]
    MissingContinuity,
    #[error("terminal-erasure continuity regressed")]
    RegressedContinuity,
    #[error("terminal-erasure histories diverged")]
    DivergedContinuity,
    #[error("invalid terminal counterparty id")]
    InvalidCounterpartyId,
    #[error("terminal-erasure sequence overflow")]
    SequenceOverflow,
    #[error("filesystem operation failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
