//! Generation continuity marker and ledger validation helpers.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use super::error::ControlError;
use super::wire::{io, mac_hex, malformed, master_key_context, verify_mac, SignedTerminalLedger};

pub(super) const MARKER_FILE: &str = ".openspine-generation";

pub(super) fn marker_path(data_root: &Path) -> PathBuf {
    data_root.join(MARKER_FILE)
}

pub(super) fn read_generation_marker(
    data_root: &Path,
    master_key: &[u8],
) -> Result<Option<String>, ControlError> {
    let path = marker_path(data_root);
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(io(&path, source)),
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(malformed)?;
    let version = value
        .get("version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| ControlError::MalformedState("missing marker version".into()))?;
    if version != 1 {
        return Err(ControlError::MalformedState(
            "unsupported marker version".into(),
        ));
    }
    let ctx = value
        .get("master_key_context")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ControlError::MalformedState("missing marker context".into()))?;
    if ctx != master_key_context(master_key) {
        return Err(ControlError::AuthenticationFailed);
    }
    let continuity_id = value
        .get("continuity_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ControlError::MalformedState("missing marker continuity_id".into()))?
        .to_owned();
    let mac = value
        .get("hmac_sha256")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ControlError::MalformedState("missing marker mac".into()))?;

    let canonical = format!("version=1;ctx={ctx};continuity_id={continuity_id}");
    verify_mac(
        b"openspine.overlay.generation-marker.v1\0",
        canonical.as_bytes(),
        master_key,
        mac,
    )?;

    Ok(Some(continuity_id))
}

pub(super) fn write_generation_marker(
    data_root: &Path,
    continuity_id: &str,
    master_key: &[u8],
) -> Result<(), ControlError> {
    let path = marker_path(data_root);
    let ctx = master_key_context(master_key);
    let canonical = format!("version=1;ctx={ctx};continuity_id={continuity_id}");
    let mac = mac_hex(
        b"openspine.overlay.generation-marker.v1\0",
        canonical.as_bytes(),
        master_key,
    );

    let obj = serde_json::json!({
        "version": 1,
        "master_key_context": ctx,
        "continuity_id": continuity_id,
        "hmac_sha256": mac,
    });
    let bytes = serde_json::to_vec(&obj).map_err(malformed)?;

    let temp = data_root.join(".generation-marker.tmp");
    let _ = fs::remove_file(&temp);

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|source| io(&temp, source))?;
    file.write_all(&bytes).map_err(|source| io(&temp, source))?;
    file.sync_all().map_err(|source| io(&temp, source))?;

    fs::rename(&temp, &path).map_err(|source| io(&path, source))?;
    super::fs::sync_dir(data_root)
}

pub(super) fn ensure_data_root_dir(data_root: &Path) -> Result<(), ControlError> {
    match fs::symlink_metadata(data_root) {
        Ok(m) if m.file_type().is_symlink() => {
            Err(ControlError::SymlinkDataRoot(data_root.to_path_buf()))
        }
        Ok(m) if !m.is_dir() => Err(ControlError::NotDirectory(data_root.to_path_buf())),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut b = fs::DirBuilder::new();
            b.recursive(true).mode(0o700);
            b.create(data_root).map_err(|source| io(data_root, source))
        }
        Err(source) => Err(io(data_root, source)),
    }
}

pub(super) fn check_continuity_alignment(
    marker_id: Option<&str>,
    ledger: Option<&SignedTerminalLedger>,
) -> Result<(), ControlError> {
    match (marker_id, ledger) {
        (Some(m), Some(l)) => {
            if m == l.continuity_id() {
                Ok(())
            } else {
                Err(ControlError::RegressedContinuity)
            }
        }
        (Some(_), None) => Err(ControlError::MissingContinuity),
        (None, Some(_)) => Err(ControlError::MissingContinuity),
        (None, None) => Ok(()),
    }
}
