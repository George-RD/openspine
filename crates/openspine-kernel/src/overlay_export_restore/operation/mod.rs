//! Restart-bound export/restore orchestration on top of control + bundle.

use std::path::Path;

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::grant::TaskGrant;
use sha2::{Digest, Sha256};

use super::control::{
    ControlError, OperationAuthorization, OperationKind, OperationStage, OverlayControl,
    PendingOperation, SignedTerminalLedger,
};
#[cfg(test)]
use super::install::write_continuity;
use super::install::{cleanup_install, remove_continuity};
use super::types::{
    CompletionMetadata, FinalizationOutcome, OverlayOperationError, OverlayOperationKind,
    PendingFinalization,
};

mod process;

/// AppState-facing controller: exclusive lock + master key for bundle ops.
pub(crate) struct OverlayOperations {
    pub(super) control: OverlayControl,
    pub(super) master_key: Vec<u8>,
}

pub(crate) fn acquire(
    data_root: &Path,
    master_key: &[u8],
) -> Result<OverlayOperations, OverlayOperationError> {
    Ok(OverlayOperations {
        control: OverlayControl::acquire(data_root, master_key)?,
        master_key: master_key.to_vec(),
    })
}

impl OverlayOperations {
    #[cfg(test)]
    pub(crate) fn acquire(
        data_root: &Path,
        master_key: &[u8],
    ) -> Result<Self, OverlayOperationError> {
        acquire(data_root, master_key)
    }

    pub(crate) fn canonical_data_root(&self) -> &Path {
        self.control.canonical_data_root()
    }
    #[cfg(test)]
    pub(crate) fn control_root(&self) -> &Path {
        self.control.control_root()
    }
    #[cfg(test)]
    pub(crate) fn snapshots_root(&self) -> &Path {
        self.control.snapshots_root()
    }

    pub(crate) fn stage_export_or_restore(
        &self,
        grant: &TaskGrant,
        action: &ActionId,
        bundle_name: &str,
        now: Timestamp,
    ) -> Result<PendingOperation, ControlError> {
        self.control
            .stage_export_or_restore(grant, action, bundle_name, now)
    }

    #[cfg(test)]
    pub(crate) fn transition_operation(
        &self,
        request_id: &str,
        stage: OperationStage,
    ) -> Result<PendingOperation, ControlError> {
        self.control.transition_operation(request_id, stage)
    }

    pub(crate) fn record_terminal_erasure(
        &self,
        counterparty_id: &str,
    ) -> Result<SignedTerminalLedger, ControlError> {
        self.control.record_terminal_erasure(counterparty_id)
    }

    pub(crate) fn export_terminal_ledger(&self) -> Result<SignedTerminalLedger, ControlError> {
        self.control.export_terminal_ledger()
    }

    #[cfg(test)]
    pub(crate) fn initialize_terminal_ledger(&self) -> Result<SignedTerminalLedger, ControlError> {
        self.control.initialize_terminal_ledger()
    }

    #[cfg(test)]
    pub(crate) fn export_portable_continuity(&self) -> Result<Vec<u8>, ControlError> {
        self.control.export_portable_continuity()
    }

    #[cfg(test)]
    pub(crate) fn import_portable_continuity(
        &self,
        portable: &[u8],
    ) -> Result<SignedTerminalLedger, OverlayOperationError> {
        let ledger = self.control.import_terminal_ledger(portable)?;
        write_continuity(
            self.control.control_root(),
            &self.control.export_portable_continuity()?,
        )?;
        Ok(ledger)
    }

    /// Process restart-bound export/restore. `rollback_requested` is pathless.
    pub(crate) fn process_pre_open(
        &self,
        rollback_requested: bool,
        _now: Timestamp,
    ) -> Result<Option<PendingFinalization>, OverlayOperationError> {
        let operation = self.control.load_operation()?;
        let Some(operation) = operation else {
            self.control.ensure_data_root_for_first_boot()?;
            let _ = self.control.initialize_terminal_ledger()?;
            return Ok(None);
        };
        let view = OperationView::from_pending(&operation);
        match view.kind {
            OperationKind::Export => self.process_export(&view, rollback_requested),
            OperationKind::Restore => self.process_restore(&view, rollback_requested),
        }
    }

    pub(crate) fn begin_finalization(
        &self,
        pending: &PendingFinalization,
        now: Timestamp,
    ) -> Result<CompletionMetadata, OverlayOperationError> {
        let current = self
            .control
            .load_operation()?
            .ok_or(ControlError::NoPendingOperation)?;
        if current.request_id() != pending.request_id {
            return Err(ControlError::RequestMismatch.into());
        }
        let stage = current.stage();
        match pending.outcome {
            FinalizationOutcome::Completed => match stage {
                OperationStage::Finalizing => {}
                OperationStage::Staged if pending.kind == OverlayOperationKind::Export => {
                    self.control
                        .transition_operation(&pending.request_id, OperationStage::Finalizing)?;
                }
                OperationStage::Installed if pending.kind == OverlayOperationKind::Restore => {
                    self.control
                        .transition_operation(&pending.request_id, OperationStage::Finalizing)?;
                }
                other => {
                    return Err(OverlayOperationError::UnrecoverableStage {
                        stage: format!("{other:?}"),
                    });
                }
            },
            FinalizationOutcome::RolledBack => match stage {
                OperationStage::RolledBack => {}
                OperationStage::RollbackRequested => {
                    self.control
                        .transition_operation(&pending.request_id, OperationStage::RolledBack)?;
                }
                other => {
                    return Err(OverlayOperationError::UnrecoverableStage {
                        stage: format!("{other:?}"),
                    });
                }
            },
        }
        Ok(CompletionMetadata {
            kind: pending.kind,
            outcome: pending.outcome,
            request_id: pending.request_id.clone(),
            action_id: pending.action_id.clone(),
            owner_principal_id: pending.owner_principal_id.clone(),
            grant_id: pending.grant_id.clone(),
            bundle_name: pending.bundle_name.clone(),
            path_digest: pending.path_digest.clone(),
            requested_at: pending.requested_at.clone(),
            completed_at: now.to_string(),
        })
    }

    pub(crate) fn complete_finalization(
        &self,
        meta: &CompletionMetadata,
    ) -> Result<(), OverlayOperationError> {
        cleanup_install(self.control.canonical_data_root(), &meta.request_id)?;
        remove_continuity(self.control.control_root())?;
        match self.control.clear_operation(&meta.request_id) {
            Ok(()) | Err(ControlError::NoPendingOperation) => Ok(()),
            Err(other) => Err(other.into()),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct OperationView {
    pub(super) kind: OperationKind,
    pub(super) stage: OperationStage,
    pub(super) request_id: String,
    pub(super) action_id: String,
    pub(super) owner_principal_id: String,
    pub(super) grant_id: String,
    pub(super) bundle_name: String,
    pub(super) requested_at: String,
    pub(super) source_bundle_request: Option<OperationAuthorization>,
}

impl OperationView {
    pub(super) fn from_pending(operation: &PendingOperation) -> Self {
        Self {
            kind: operation.kind(),
            stage: operation.stage(),
            request_id: operation.request_id().to_owned(),
            action_id: operation.action_id().to_owned(),
            owner_principal_id: operation.owner_principal_id().to_owned(),
            grant_id: operation.grant_id().to_owned(),
            bundle_name: operation.bundle_name().as_str().to_owned(),
            requested_at: operation.requested_at().to_owned(),
            source_bundle_request: operation.source_bundle_request().cloned(),
        }
    }
}

pub(super) fn pending_from(
    operation: &OperationView,
    outcome: FinalizationOutcome,
) -> PendingFinalization {
    PendingFinalization {
        kind: match operation.kind {
            OperationKind::Export => OverlayOperationKind::Export,
            OperationKind::Restore => OverlayOperationKind::Restore,
        },
        outcome,
        request_id: operation.request_id.clone(),
        action_id: operation.action_id.clone(),
        owner_principal_id: operation.owner_principal_id.clone(),
        grant_id: operation.grant_id.clone(),
        bundle_name: operation.bundle_name.clone(),
        path_digest: digest_hex(operation.bundle_name.as_bytes()),
        requested_at: operation.requested_at.clone(),
    }
}

fn digest_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    const D: &[u8; 16] = b"0123456789abcdef";
    for &b in digest.as_slice() {
        out.push(D[(b >> 4) as usize] as char);
        out.push(D[(b & 15) as usize] as char);
    }
    out
}
