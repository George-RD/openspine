//! Export/restore pre-open processing for OverlayOperations.

use super::super::bundle::{
    self, apply_terminal_erasure_delta, BundleManifest, BundleRequestMetadata,
    TerminalLedgerBaseline,
};
use super::super::control::{ControlError, OperationStage, SignedTerminalLedger, TerminalLedger};
use super::super::install::{
    cleanup_install, inspect_install, install_or_recover, load_continuity, rollback_or_recover,
    staged_path, write_continuity, InstallState,
};
use super::super::types::{FinalizationOutcome, OverlayOperationError, PendingFinalization};
use super::{pending_from, OperationView, OverlayOperations};

impl OverlayOperations {
    pub(super) fn process_export(
        &self,
        operation: &OperationView,
        rollback_flag: bool,
    ) -> Result<Option<PendingFinalization>, OverlayOperationError> {
        if rollback_flag {
            return Err(OverlayOperationError::UnrecoverableStage {
                stage: format!("{:?}", operation.stage),
            });
        }
        let _ = self.control.initialize_terminal_ledger()?;
        match operation.stage {
            OperationStage::Requested | OperationStage::Staged => {
                self.publish_export_if_needed(operation)?;
                let current = if operation.stage == OperationStage::Requested {
                    OperationView::from_pending(
                        &self
                            .control
                            .transition_operation(&operation.request_id, OperationStage::Staged)?,
                    )
                } else {
                    operation.clone()
                };
                Ok(Some(pending_from(&current, FinalizationOutcome::Completed)))
            }
            OperationStage::Finalizing => Ok(Some(pending_from(
                operation,
                FinalizationOutcome::Completed,
            ))),
            other => Err(OverlayOperationError::UnrecoverableStage {
                stage: format!("{other:?}"),
            }),
        }
    }

    fn publish_export_if_needed(
        &self,
        operation: &OperationView,
    ) -> Result<(), OverlayOperationError> {
        let bundle_path = self.control.snapshots_root().join(&operation.bundle_name);
        if bundle_path.exists() {
            let manifest = bundle::verify_named_bundle(
                &bundle_path,
                &operation.bundle_name,
                self.master_key.as_slice(),
            )?;
            let req = manifest.request();
            if req.request_id != operation.request_id
                || req.action_id != operation.action_id
                || req.owner_principal_id != operation.owner_principal_id
                || req.grant_id != operation.grant_id
                || req.requested_at != operation.requested_at
            {
                return Err(bundle::BundleError::InvalidManifest(
                    "existing export bundle metadata mismatch".into(),
                )
                .into());
            }
            bundle::sync_existing_bundle(self.control.snapshots_root(), &operation.bundle_name)?;
            return Ok(());
        }
        let ledger = self.control.export_terminal_ledger()?;
        let request = BundleRequestMetadata {
            request_id: operation.request_id.clone(),
            action_id: operation.action_id.clone(),
            owner_principal_id: operation.owner_principal_id.clone(),
            grant_id: operation.grant_id.clone(),
            requested_at: operation.requested_at.clone(),
        };
        let baseline = TerminalLedgerBaseline {
            continuity_id: ledger.continuity_id().to_owned(),
            sequence: ledger.sequence(),
            erased_counterparty_ids: ledger.erased_counterparty_ids().iter().cloned().collect(),
            ledger_hmac_sha256: ledger.hmac_sha256().to_owned(),
        };
        let _ = bundle::publish_bundle(
            self.control.snapshots_root(),
            &operation.bundle_name,
            self.control.canonical_data_root(),
            request,
            baseline,
            self.master_key.as_slice(),
        )?;
        Ok(())
    }

    pub(super) fn process_restore(
        &self,
        operation: &OperationView,
        rollback_flag: bool,
    ) -> Result<Option<PendingFinalization>, OverlayOperationError> {
        let data_root = self.control.canonical_data_root().to_path_buf();
        let bundle_path = self.control.snapshots_root().join(&operation.bundle_name);
        let mut stage = operation.stage;
        if rollback_flag {
            match stage {
                OperationStage::Requested | OperationStage::Staged => {
                    return Err(OverlayOperationError::UnrecoverableStage {
                        stage: format!("{stage:?}"),
                    });
                }
                OperationStage::Installed | OperationStage::Finalizing => {
                    let install_state = inspect_install(&data_root, &operation.request_id)?;
                    match install_state {
                        InstallState::Swapped | InstallState::PreviousOnly => {}
                        InstallState::Clean
                        | InstallState::StagedOnly
                        | InstallState::CleanupCommitted => {
                            return Err(OverlayOperationError::MissingInstallState);
                        }
                        InstallState::Ambiguous => {
                            return Err(OverlayOperationError::AmbiguousInstallState);
                        }
                    }
                    stage = self
                        .control
                        .transition_operation(
                            &operation.request_id,
                            OperationStage::RollbackRequested,
                        )?
                        .stage();
                }
                OperationStage::RollbackRequested | OperationStage::RolledBack => {}
            }
        }
        match stage {
            OperationStage::Requested => {
                let manifest = bundle::verify_named_bundle(
                    &bundle_path,
                    &operation.bundle_name,
                    self.master_key.as_slice(),
                )?;
                let Some(source) = operation.source_bundle_request.as_ref() else {
                    return Err(bundle::BundleError::InvalidManifest(
                        "restore operation is missing authenticated source request metadata".into(),
                    )
                    .into());
                };
                let request = manifest.request();
                if request.request_id != source.request_id
                    || request.action_id != source.action_id
                    || request.owner_principal_id != source.owner_principal_id
                    || request.grant_id != source.grant_id
                    || request.requested_at != source.requested_at
                {
                    return Err(bundle::BundleError::InvalidManifest(
                        "restore source request metadata mismatch".into(),
                    )
                    .into());
                }
                let portable = self.established_portable_continuity()?;
                let merged = self.harden_merged_ledger(&manifest, &portable)?;
                let staged = staged_path(&data_root, &operation.request_id);
                if staged.exists() {
                    cleanup_install(&data_root, &operation.request_id)?;
                }
                let _ = bundle::stage_bundle(&bundle_path, &staged, self.master_key.as_slice())?;
                let all_merged_ids: Vec<String> =
                    merged.erased_counterparty_ids().iter().cloned().collect();
                apply_terminal_erasure_delta(&staged, &manifest, &all_merged_ids)?;
                write_continuity(
                    self.control.control_root(),
                    &self.control.export_portable_continuity()?,
                )?;
                let staged_op = OperationView::from_pending(
                    &self
                        .control
                        .transition_operation(&operation.request_id, OperationStage::Staged)?,
                );
                self.install_restore(&staged_op)
            }
            OperationStage::Staged => self.install_restore(operation),
            OperationStage::Installed | OperationStage::Finalizing => Ok(Some(pending_from(
                operation,
                FinalizationOutcome::Completed,
            ))),
            OperationStage::RollbackRequested => {
                let install_state = inspect_install(&data_root, &operation.request_id)?;
                match install_state {
                    InstallState::Swapped
                    | InstallState::PreviousOnly
                    | InstallState::StagedOnly => {}
                    InstallState::Clean | InstallState::CleanupCommitted => {
                        return Err(OverlayOperationError::MissingInstallState);
                    }
                    InstallState::Ambiguous => {
                        return Err(OverlayOperationError::AmbiguousInstallState);
                    }
                }
                rollback_or_recover(&data_root, &operation.request_id)?;
                let rolled = OperationView::from_pending(
                    &self
                        .control
                        .transition_operation(&operation.request_id, OperationStage::RolledBack)?,
                );
                if let Some(bytes) = load_continuity(self.control.control_root())? {
                    let _ = self.control.import_terminal_ledger(&bytes)?;
                }
                Ok(Some(pending_from(&rolled, FinalizationOutcome::RolledBack)))
            }
            OperationStage::RolledBack => Ok(Some(pending_from(
                operation,
                FinalizationOutcome::RolledBack,
            ))),
        }
    }

    fn install_restore(
        &self,
        operation: &OperationView,
    ) -> Result<Option<PendingFinalization>, OverlayOperationError> {
        install_or_recover(self.control.canonical_data_root(), &operation.request_id)?;
        let installed = if operation.stage == OperationStage::Installed {
            operation.clone()
        } else {
            OperationView::from_pending(
                &self
                    .control
                    .transition_operation(&operation.request_id, OperationStage::Installed)?,
            )
        };
        Ok(Some(pending_from(
            &installed,
            FinalizationOutcome::Completed,
        )))
    }

    fn established_portable_continuity(&self) -> Result<Vec<u8>, OverlayOperationError> {
        if let Some(bytes) = load_continuity(self.control.control_root())? {
            if self.control.export_terminal_ledger().is_err() {
                let _ = self.control.import_terminal_ledger(&bytes)?;
            }
            return Ok(bytes);
        }
        // Same-host established local ledger; fresh host must import first.
        Ok(self.control.export_portable_continuity()?)
    }

    fn harden_merged_ledger(
        &self,
        manifest: &BundleManifest,
        portable: &[u8],
    ) -> Result<SignedTerminalLedger, OverlayOperationError> {
        let baseline = manifest.ledger_baseline();
        if baseline.continuity_id.is_empty() {
            return Err(ControlError::MissingContinuity.into());
        }
        let body = TerminalLedger::with_continuity_id(
            &baseline.continuity_id,
            baseline.sequence,
            baseline.erased_counterparty_ids.iter().cloned().collect(),
        );
        let expected = self.control.sign_ledger(body)?;
        if expected.hmac_sha256() != baseline.ledger_hmac_sha256 {
            return Err(ControlError::AuthenticationFailed.into());
        }
        let signed = SignedTerminalLedger::with_continuity_id(
            &baseline.continuity_id,
            baseline.sequence,
            baseline.erased_counterparty_ids.iter().cloned().collect(),
            baseline.ledger_hmac_sha256.clone(),
        )?;
        Ok(self.control.merge_bundle_baseline(&signed, portable)?)
    }
}
