//! Package-selection decision and receipt payloads (ANO-822).
//!
//! These are Store-persisted, package-specific, channel-neutral records. None of
//! these values is a bearer permission: a decision authorizes one exact
//! transition only when the Store consumes it in the commit transaction.
use super::install_types::PackageIdentity;
use jiff::Timestamp;
use openspine_schemas::digest::Digest;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

/// A decision may never stay valid for longer than this.
pub(crate) const MAX_DECISION_LIFETIME_SECS: i64 = 30 * 60;
/// Bound on persisted consequences so one decision stays reviewable.
pub(crate) const MAX_RECONFIRMATIONS: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SelectionSourceMode {
    /// No selection history: the base the operator configured as `lyra_dir`.
    LegacyConfigured,
    /// The ledger-backed selection of an earlier generation.
    Managed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SelectionSource {
    pub mode: SelectionSourceMode,
    pub identity: PackageIdentity,
    /// Present exactly for [`SelectionSourceMode::Managed`].
    pub installation_id: Option<Ulid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SelectionTarget {
    pub installation_id: Ulid,
    pub identity: PackageIdentity,
}

/// A learned artifact the transition conservatively sends back through the
/// normal reconfirmation lifecycle before reuse.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReconfirmationKey {
    pub kind: String,
    pub artifact_id: String,
    pub version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SelectionDecision {
    pub schema_version: u32,
    pub decision_id: Ulid,
    /// Domain-separated MAC over the persisted owner principal/identity IDs
    /// under the protected artifact key (computed by the caller).
    pub owner_binding: String,
    /// The persisted owner principal that accepts the transition.
    pub principal: String,
    pub source: SelectionSource,
    pub target: SelectionTarget,
    pub plan_digest: Digest,
    /// Generation the selection holds when the plan is evaluated (0 = legacy).
    pub expected_generation: u64,
    pub runtime_compatibility: Digest,
    /// Overlay/provenance/live-control binding captured with the plan.
    pub control_fingerprint: Digest,
    pub base_epoch_changed: bool,
    /// Sorted, duplicate-free rows to flip to `reconfirmation_required`.
    pub reconfirmations: Vec<ReconfirmationKey>,
    pub presented_at: Timestamp,
    pub expires_at: Timestamp,
}

/// The one committed projection of the audited history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectionRecord {
    pub generation: u64,
    pub invalidation_generation: u64,
    pub installation_id: Ulid,
}

/// Authenticated audit payload of `package.selection_committed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SelectionCommitted {
    pub generation: u64,
    pub invalidation_generation: u64,
    pub decision: SelectionDecision,
}

/// Fixed refusal reasons. They never echo package or owner content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SelectionRefusal {
    #[error("the decision is malformed or exceeds supported bounds")]
    Malformed,
    #[error("the selection generation changed since this review")]
    GenerationChanged,
    #[error("the selected source no longer matches this review")]
    SourceChanged,
    #[error("the target installation is not a committed installation of that identity")]
    TargetUnavailable,
    #[error("no matching presented decision exists")]
    UnknownDecision,
    #[error("the decision was already consumed")]
    AlreadyConsumed,
    #[error("the decision expired")]
    Expired,
    #[error("the confirmed decision differs from the presented decision")]
    Mismatch,
    #[error("a required reconfirmation consequence no longer applies")]
    ConsequenceStale,
}
