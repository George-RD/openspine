//! Manifest wire format, validation, and HMAC authentication for bundles.

use hmac_sha256::HMAC;
use serde::{Deserialize, Serialize};
use std::io::Read as _;
use std::path::Path;

use super::durable::{io_err, open_regular_nofollow, same_state};
use super::tree::validate_entries;
use super::{
    BundleEntry, BundleError, BundleRequestMetadata, TerminalLedgerBaseline, BUNDLE_VERSION,
    DATA_DIR,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in super::super) struct BundleManifest {
    pub(super) body: ManifestBody,
    pub(super) hmac_sha256: String,
}

impl BundleManifest {
    pub(in super::super) fn bundle_name(&self) -> &str {
        &self.body.bundle_name
    }

    pub(in super::super) fn request(&self) -> &BundleRequestMetadata {
        &self.body.request
    }

    pub(in super::super) fn ledger_baseline(&self) -> &TerminalLedgerBaseline {
        &self.body.terminal_ledger_baseline
    }

    pub(in super::super) fn entries(&self) -> &[BundleEntry] {
        &self.body.entries
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ManifestBody {
    pub(super) version: u32,
    pub(super) bundle_name: String,
    pub(super) request: BundleRequestMetadata,
    pub(super) terminal_ledger_baseline: TerminalLedgerBaseline,
    pub(super) entries: Vec<BundleEntry>,
}

pub(super) fn sign_manifest(body: ManifestBody, key: &[u8]) -> Result<BundleManifest, BundleError> {
    let canonical = serde_json::to_vec(&body)
        .map_err(|error| BundleError::InvalidManifest(error.to_string()))?;
    Ok(BundleManifest {
        body,
        hmac_sha256: hex(&HMAC::mac(canonical, key)),
    })
}

pub(super) fn read_manifest(path: &Path, key: &[u8]) -> Result<BundleManifest, BundleError> {
    if key.is_empty() {
        return Err(BundleError::InvalidHmac);
    }
    let mut file = open_regular_nofollow(path, "open manifest")?;
    let before = file
        .metadata()
        .map_err(|source| io_err("inspect manifest", path, source))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|source| io_err("read manifest", path, source))?;
    let after = file
        .metadata()
        .map_err(|source| io_err("reinspect manifest", path, source))?;
    if !same_state(&before, &after) {
        return Err(BundleError::ConcurrentMutation(path.to_path_buf()));
    }
    let manifest: BundleManifest = serde_json::from_slice(&bytes)
        .map_err(|error| BundleError::InvalidManifest(error.to_string()))?;
    let canonical = serde_json::to_vec(&manifest)
        .map_err(|error| BundleError::InvalidManifest(error.to_string()))?;
    if canonical != bytes {
        return Err(BundleError::InvalidManifest(
            "manifest is not canonical JSON".into(),
        ));
    }
    validate_body(&manifest.body)?;
    let body = serde_json::to_vec(&manifest.body)
        .map_err(|error| BundleError::InvalidManifest(error.to_string()))?;
    let tag = decode_hex_32(&manifest.hmac_sha256).ok_or(BundleError::InvalidHmac)?;
    if !HMAC::verify(body, key, &tag) {
        return Err(BundleError::InvalidHmac);
    }
    Ok(manifest)
}

pub(super) fn validate_body(body: &ManifestBody) -> Result<(), BundleError> {
    if body.version != BUNDLE_VERSION {
        return Err(BundleError::InvalidManifest("unsupported version".into()));
    }
    validate_bundle_name(&body.bundle_name)?;
    if body.request.action_id != "openspine.overlay.export"
        || [
            &body.request.request_id,
            &body.request.owner_principal_id,
            &body.request.grant_id,
            &body.request.requested_at,
        ]
        .iter()
        .any(|value| value.is_empty())
    {
        return Err(BundleError::InvalidManifest(
            "invalid request metadata".into(),
        ));
    }
    let ledger = &body.terminal_ledger_baseline;
    if !valid_continuity_id(&ledger.continuity_id)
        || decode_hex_32(&ledger.ledger_hmac_sha256).is_none()
        || ledger.erased_counterparty_ids.iter().any(String::is_empty)
        || !ledger
            .erased_counterparty_ids
            .windows(2)
            .all(|pair| pair[0] < pair[1])
    {
        return Err(BundleError::InvalidManifest(
            "invalid terminal ledger baseline".into(),
        ));
    }
    validate_entries(&body.entries)
}

fn valid_continuity_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 && value.bytes().all(|b| b.is_ascii_alphanumeric())
}

pub(super) fn validate_bundle_name(name: &str) -> Result<(), BundleError> {
    if name.is_empty()
        || name.len() > 128
        || name.starts_with('.')
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(BundleError::InvalidBundleName);
    }
    Ok(())
}

pub(super) fn validate_manifest_path(path: &str) -> Result<(), BundleError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.ends_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || (path != DATA_DIR && !path.starts_with("data/"))
    {
        return Err(BundleError::InvalidManifest(format!(
            "non-normal path: {path}"
        )));
    }
    Ok(())
}

pub(super) fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

pub(super) fn decode_hex_32(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut out = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        out[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    Some(out)
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}
