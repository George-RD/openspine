//! Overlay export/restore control plane: exclusive lifetime lock, pending
//! operation staging, terminal-erasure ledger, and portable continuity.

use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::grant::TaskGrant;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

mod acquire;
mod continuity;
mod error;
mod fs;
mod generation;
mod ledger;
mod wire;

use acquire::{
    cleanup_temp_at, open_canonical_dir_nofollow, open_or_create_control_dir_relative,
    openat_file_nofollow, resolve_root_identity, secure_dir_fd, secure_or_create_sub_dir_at,
    try_lock_exclusive, verify_entry_is_nofollow_dir,
};
use fs::{atomic_write, remove_durable};
use ledger::valid_transition;
use wire::{io, mac_hex, malformed, verify_mac, SignedOperation, OPERATION_DOMAIN};

pub(crate) use error::ControlError;
#[cfg(test)]
pub(super) use wire::LEDGER_FILE;
pub(crate) use wire::{
    OperationAuthorization, OperationKind, OperationStage, PendingOperation, SignedTerminalLedger,
    TerminalLedger,
};
pub(super) use wire::{
    EXPORT_ACTION, FORMAT_VERSION, LEDGER_TEMP, OPERATION_FILE, OPERATION_TEMP, RESTORE_ACTION,
};

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct BundleName(String);

impl BundleName {
    pub(crate) fn parse(value: &str) -> Result<Self, ControlError> {
        let ok = !value.is_empty()
            && value.len() <= 128
            && !value.starts_with('.')
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        ok.then(|| Self(value.to_owned()))
            .ok_or(ControlError::InvalidBundleName)
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) struct OverlayControl {
    canonical_data_root: PathBuf,
    control_root: PathBuf,
    snapshots_root: PathBuf,
    master_key: Vec<u8>,
    _control_dir: File,
    _lifetime_lock: File,
    state: Mutex<()>,
    #[cfg(test)]
    fail_before_first_boot_marker: std::sync::atomic::AtomicBool,
    #[cfg(test)]
    fail_before_init_ledger_marker: std::sync::atomic::AtomicBool,
}

impl OverlayControl {
    pub(crate) fn acquire(data_root: &Path, master_key: &[u8]) -> Result<Self, ControlError> {
        if master_key.is_empty() {
            return Err(ControlError::AuthenticationFailed);
        }

        let identity = resolve_root_identity(data_root)?;
        let parent_dir = open_canonical_dir_nofollow(&identity.canonical_parent)?;
        let control_dir = open_or_create_control_dir_relative(
            parent_dir.as_raw_fd(),
            &identity.control_root_name,
            &identity.control_root,
        )?;
        let lock = openat_file_nofollow(control_dir.as_raw_fd(), "lifetime.lock", true)
            .map_err(|source| io(&identity.control_root.join("lifetime.lock"), source))?;

        try_lock_exclusive(&lock, &identity.control_root.join("lifetime.lock"))?;

        secure_dir_fd(&control_dir, &identity.control_root)?;
        let snapshots_root = identity.control_root.join("snapshots");
        let _snapshots_dir =
            secure_or_create_sub_dir_at(control_dir.as_raw_fd(), "snapshots", &snapshots_root)?;
        cleanup_temp_at(
            control_dir.as_raw_fd(),
            OPERATION_TEMP,
            &identity.control_root.join(OPERATION_TEMP),
        )?;
        cleanup_temp_at(
            control_dir.as_raw_fd(),
            LEDGER_TEMP,
            &identity.control_root.join(LEDGER_TEMP),
        )?;

        Ok(Self {
            canonical_data_root: identity.canonical_data_root,
            control_root: identity.control_root,
            snapshots_root,
            master_key: master_key.to_vec(),
            _control_dir: control_dir,
            _lifetime_lock: lock,
            state: Mutex::new(()),
            #[cfg(test)]
            fail_before_first_boot_marker: std::sync::atomic::AtomicBool::new(false),
            #[cfg(test)]
            fail_before_init_ledger_marker: std::sync::atomic::AtomicBool::new(false),
        })
    }

    pub(crate) fn canonical_data_root(&self) -> &Path {
        &self.canonical_data_root
    }

    pub(crate) fn control_root(&self) -> &Path {
        &self.control_root
    }

    pub(crate) fn snapshots_root(&self) -> &Path {
        &self.snapshots_root
    }

    #[cfg(test)]
    pub(crate) fn bundle_path(&self, name: &BundleName) -> PathBuf {
        self.snapshots_root.join(name.as_str())
    }

    pub(crate) fn stage_export_or_restore(
        &self,
        grant: &TaskGrant,
        action: &ActionId,
        bundle_name: &str,
        now: Timestamp,
    ) -> Result<PendingOperation, ControlError> {
        let kind = match action.as_str() {
            EXPORT_ACTION => OperationKind::Export,
            RESTORE_ACTION => OperationKind::Restore,
            other => return Err(ControlError::InvalidAction(other.to_owned())),
        };
        self.stage_operation(
            kind,
            BundleName::parse(bundle_name)?,
            OperationAuthorization {
                action_id: action.to_string(),
                owner_principal_id: grant.user.clone(),
                grant_id: grant.id.to_string(),
                request_id: Ulid::new().to_string(),
                requested_at: now.to_string(),
            },
        )
    }

    pub(crate) fn stage_operation(
        &self,
        kind: OperationKind,
        bundle_name: BundleName,
        authorization: OperationAuthorization,
    ) -> Result<PendingOperation, ControlError> {
        let _g = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if self
            .operation_path()
            .try_exists()
            .map_err(|source| io(&self.operation_path(), source))?
        {
            return Err(ControlError::OperationPending);
        }

        let snapshots_fd = open_canonical_dir_nofollow(&self.snapshots_root)?;
        let source_bundle_request = match kind {
            OperationKind::Export => {
                let path = self.snapshots_root.join(bundle_name.as_str());
                match path.symlink_metadata() {
                    Ok(_) => return Err(ControlError::UnsafeControlPath(path)),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(source) => return Err(io(&path, source)),
                }
                None
            }
            OperationKind::Restore => {
                let _entry_fd =
                    verify_entry_is_nofollow_dir(snapshots_fd.as_raw_fd(), bundle_name.as_str())
                        .map_err(|_| {
                            ControlError::NotDirectory(
                                self.snapshots_root.join(bundle_name.as_str()),
                            )
                        })?;
                let bundle_dir = self.snapshots_root.join(bundle_name.as_str());
                let manifest = super::bundle::verify_named_bundle(
                    &bundle_dir,
                    bundle_name.as_str(),
                    &self.master_key,
                )
                .map_err(|_| ControlError::AuthenticationFailed)?;
                let _recheck_fd =
                    verify_entry_is_nofollow_dir(snapshots_fd.as_raw_fd(), bundle_name.as_str())
                        .map_err(|_| {
                            ControlError::NotDirectory(
                                self.snapshots_root.join(bundle_name.as_str()),
                            )
                        })?;
                let req = manifest.request();
                Some(OperationAuthorization {
                    action_id: req.action_id.clone(),
                    owner_principal_id: req.owner_principal_id.clone(),
                    grant_id: req.grant_id.clone(),
                    request_id: req.request_id.clone(),
                    requested_at: req.requested_at.clone(),
                })
            }
        };

        let expected = match kind {
            OperationKind::Export => EXPORT_ACTION,
            OperationKind::Restore => RESTORE_ACTION,
        };
        if authorization.action_id != expected {
            return Err(ControlError::InvalidAction(authorization.action_id));
        }

        let operation = PendingOperation {
            version: FORMAT_VERSION,
            kind,
            bundle_name,
            authorization,
            source_bundle_request,
            stage: OperationStage::Requested,
        };
        self.write_operation(&operation)?;
        Ok(operation)
    }

    pub(crate) fn load_operation(&self) -> Result<Option<PendingOperation>, ControlError> {
        let _g = self.state.lock().unwrap_or_else(|p| p.into_inner());
        self.load_operation_unlocked()
    }

    pub(crate) fn transition_operation(
        &self,
        request_id: &str,
        next: OperationStage,
    ) -> Result<PendingOperation, ControlError> {
        let _g = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let mut operation = self
            .load_operation_unlocked()?
            .ok_or(ControlError::NoPendingOperation)?;
        if operation.request_id() != request_id {
            return Err(ControlError::RequestMismatch);
        }
        if !valid_transition(operation.kind, operation.stage, next) {
            return Err(ControlError::InvalidTransition);
        }
        operation.stage = next;
        self.write_operation(&operation)?;
        Ok(operation)
    }

    pub(crate) fn clear_operation(&self, request_id: &str) -> Result<(), ControlError> {
        let _g = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let operation = self
            .load_operation_unlocked()?
            .ok_or(ControlError::NoPendingOperation)?;
        if operation.request_id() != request_id {
            return Err(ControlError::RequestMismatch);
        }
        if !matches!(
            operation.stage,
            OperationStage::Finalizing | OperationStage::RolledBack
        ) {
            return Err(ControlError::InvalidTransition);
        }
        remove_durable(&self.operation_path(), &self.control_root)
    }

    fn operation_path(&self) -> PathBuf {
        self.control_root.join(OPERATION_FILE)
    }

    fn load_operation_unlocked(&self) -> Result<Option<PendingOperation>, ControlError> {
        let path = self.operation_path();
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(io(&path, source)),
        };
        let signed: SignedOperation = serde_json::from_slice(&bytes).map_err(malformed)?;
        if signed.body.version != FORMAT_VERSION {
            return Err(ControlError::MalformedState(
                "unsupported operation version".into(),
            ));
        }
        let canonical = serde_json::to_vec(&signed.body).map_err(malformed)?;
        verify_mac(
            OPERATION_DOMAIN,
            &canonical,
            &self.master_key,
            &signed.hmac_sha256,
        )?;
        Ok(Some(signed.body))
    }

    fn write_operation(&self, operation: &PendingOperation) -> Result<(), ControlError> {
        let canonical = serde_json::to_vec(operation).map_err(malformed)?;
        let signed = SignedOperation {
            body: operation.clone(),
            hmac_sha256: mac_hex(OPERATION_DOMAIN, &canonical, &self.master_key),
        };
        atomic_write(
            &self.control_root,
            OPERATION_TEMP,
            OPERATION_FILE,
            &serde_json::to_vec(&signed).map_err(malformed)?,
        )
    }
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;
