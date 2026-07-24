//! Wire types, HMAC helpers, and hex encoding for overlay control state.

use std::path::Path;

use hmac_sha256::HMAC;
use serde::{Deserialize, Serialize};

use super::ControlError;

pub(crate) const FORMAT_VERSION: u32 = 1;
pub(crate) const EXPORT_ACTION: &str = "openspine.overlay.export";
pub(crate) const RESTORE_ACTION: &str = "openspine.overlay.restore";
pub(crate) const OPERATION_FILE: &str = "pending-operation.json";
pub(crate) const OPERATION_TEMP: &str = ".pending-operation.tmp";
pub(crate) const LEDGER_FILE: &str = "terminal-erasure-ledger.json";
pub(crate) const LEDGER_TEMP: &str = ".terminal-erasure-ledger.tmp";
pub(super) const OPERATION_DOMAIN: &[u8] = b"openspine.overlay.operation.v1\0";
pub(super) const LEDGER_DOMAIN: &[u8] = b"openspine.overlay.terminal-ledger.v1\0";
pub(super) const PORTABLE_DOMAIN: &[u8] = b"openspine.overlay.portable-continuity.v1\0";
pub(super) const KEY_CONTEXT_DOMAIN: &[u8] = b"openspine.overlay.master-key-context.v1\0";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OperationKind {
    Export,
    Restore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OperationStage {
    Requested,
    Staged,
    Installed,
    Finalizing,
    RollbackRequested,
    RolledBack,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OperationAuthorization {
    pub(crate) action_id: String,
    pub(crate) owner_principal_id: String,
    pub(crate) grant_id: String,
    pub(crate) request_id: String,
    pub(crate) requested_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PendingOperation {
    pub(super) version: u32,
    pub(super) kind: OperationKind,
    pub(super) bundle_name: super::BundleName,
    pub(super) authorization: OperationAuthorization,
    pub(super) source_bundle_request: Option<OperationAuthorization>,
    pub(super) stage: OperationStage,
}

impl PendingOperation {
    pub(crate) fn kind(&self) -> OperationKind {
        self.kind
    }

    pub(crate) fn bundle_name(&self) -> &super::BundleName {
        &self.bundle_name
    }

    pub(crate) fn stage(&self) -> OperationStage {
        self.stage
    }

    pub(crate) fn action_id(&self) -> &str {
        &self.authorization.action_id
    }

    pub(crate) fn owner_principal_id(&self) -> &str {
        &self.authorization.owner_principal_id
    }

    pub(crate) fn grant_id(&self) -> &str {
        &self.authorization.grant_id
    }

    pub(crate) fn request_id(&self) -> &str {
        &self.authorization.request_id
    }

    pub(crate) fn requested_at(&self) -> &str {
        &self.authorization.requested_at
    }

    pub(crate) fn source_bundle_request(&self) -> Option<&OperationAuthorization> {
        self.source_bundle_request.as_ref()
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SignedOperation {
    pub(super) body: PendingOperation,
    pub(super) hmac_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TerminalLedger {
    pub(super) version: u32,
    pub(super) continuity_id: String,
    pub(super) sequence: u64,
    pub(super) erased_counterparty_ids: std::collections::BTreeSet<String>,
}

impl TerminalLedger {
    pub(crate) fn with_continuity_id(
        continuity_id: impl Into<String>,
        sequence: u64,
        erased_counterparty_ids: std::collections::BTreeSet<String>,
    ) -> Self {
        Self {
            version: FORMAT_VERSION,
            continuity_id: continuity_id.into(),
            sequence,
            erased_counterparty_ids,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SignedTerminalLedger {
    pub(super) body: TerminalLedger,
    pub(super) hmac_sha256: String,
}

impl SignedTerminalLedger {
    pub(crate) fn with_continuity_id(
        continuity_id: impl Into<String>,
        sequence: u64,
        erased_counterparty_ids: std::collections::BTreeSet<String>,
        hmac_sha256: String,
    ) -> Result<Self, ControlError> {
        Ok(Self {
            body: TerminalLedger::with_continuity_id(
                continuity_id,
                sequence,
                erased_counterparty_ids,
            ),
            hmac_sha256,
        })
    }

    pub(crate) fn continuity_id(&self) -> &str {
        &self.body.continuity_id
    }

    pub(crate) fn sequence(&self) -> u64 {
        self.body.sequence
    }

    pub(crate) fn erased_counterparty_ids(&self) -> &std::collections::BTreeSet<String> {
        &self.body.erased_counterparty_ids
    }

    pub(crate) fn hmac_sha256(&self) -> &str {
        &self.hmac_sha256
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PortableContinuityBody {
    pub(super) version: u32,
    pub(super) master_key_context: String,
    pub(super) ledger: SignedTerminalLedger,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PortableContinuity {
    pub(super) body: PortableContinuityBody,
    pub(super) hmac_sha256: String,
}

pub(super) fn master_key_context(key: &[u8]) -> String {
    mac_hex(KEY_CONTEXT_DOMAIN, b"", key)
}

pub(super) fn mac_hex(domain: &[u8], canonical: &[u8], key: &[u8]) -> String {
    let mut message = Vec::with_capacity(domain.len() + canonical.len());
    message.extend_from_slice(domain);
    message.extend_from_slice(canonical);
    hex(&HMAC::mac(message, key))
}

pub(super) fn verify_mac(
    domain: &[u8],
    canonical: &[u8],
    key: &[u8],
    supplied: &str,
) -> Result<(), ControlError> {
    let tag = decode_hex_32(supplied).ok_or(ControlError::AuthenticationFailed)?;
    let mut message = Vec::with_capacity(domain.len() + canonical.len());
    message.extend_from_slice(domain);
    message.extend_from_slice(canonical);
    if HMAC::verify(message, key, &tag) {
        Ok(())
    } else {
        Err(ControlError::AuthenticationFailed)
    }
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
pub(super) fn io(path: &Path, source: std::io::Error) -> ControlError {
    ControlError::Io {
        path: path.to_path_buf(),
        source,
    }
}

pub(super) fn malformed(source: impl std::fmt::Display) -> ControlError {
    ControlError::MalformedState(source.to_string())
}
