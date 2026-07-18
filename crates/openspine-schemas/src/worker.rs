//! Worker runtime domain types (AD-035 / AD-101 / AD-033).
//!
//! These describe the master→worker commissioning contract and the
//! worker→master result chokepoint. A commissioned worker runs under a
//! sub-grant of the master's own grant (a caveat chain, AD-101); its result
//! returns as a structured bus event, and every free-text field it carries
//! is wrapped as untrusted cargo (AD-033) — never as an executable
//! instruction.

use serde::{Deserialize, Serialize};

use crate::action::ActionId;
use crate::artifact::ArtifactRef;
/// The structured outcome a worker reports back. Mirrors AD-033's
/// worker→master crossing: schema-checked fields plus free text that stays
/// wrapped-as-untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkerOutcome {
    /// The task completed nominally.
    Completed,
    /// The task could not be completed and requires owner attention.
    Failed,
    /// The task is awaiting an external dependency (e.g. an owner approval)
    /// and will resume later.
    Awaiting,
}

/// A single structured request a worker raises to the master (AD-033's
/// `requests[]`): an ask for a resource, approval, or top-up. Free text is
/// carried out-of-band as an artifact ref, never inline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkerRequest {
    pub kind: String,
    /// Digest reference to an encrypted artifact holding the untrusted detail
    /// text (NOT a bare ULID) — cargo, never instructions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_ref: Option<ArtifactRef>,
}

/// A concrete slot the worker offers the owner (AD-033's `offered_slots[]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkerSlot {
    pub id: String,
    pub label: String,
}

/// The structured result a commissioned worker returns to the master. This
/// is the ONLY outbound channel for worker output (AD-035 reply chokepoint):
/// the worker never egresses directly — it reports this and the master
/// relays through its own gated reply path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkerResult {
    pub outcome: WorkerOutcome,
    /// Structured slots the worker offers the owner to choose from.
    #[serde(default)]
    pub offered_slots: Vec<WorkerSlot>,
    /// Structured asks the worker raises to the master.
    #[serde(default)]
    pub requests: Vec<WorkerRequest>,
    /// Digest reference to an encrypted artifact holding untrusted free-text
    /// notes (AD-033): cargo, never instructions. The master must route any
    /// such text through the prompt untrusted-context wrapper before a model
    /// call — it is never authoritative. Stored as an `ArtifactRef` (digest),
    /// never a bare ULID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_ref: Option<ArtifactRef>,
}

/// How the master scopes a commissioned worker's authority (AD-101
/// attenuation). Every field only NARROWS the master's own grant; the
/// minting function rejects any widening.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkerCommissionSpec {
    /// Agent the worker runs as (routing target inside the shell).
    pub agent_id: String,
    /// Actions the worker may perform — MUST be a subset of the master's
    /// granted actions (narrowing only).
    pub allowed_actions: Vec<ActionId>,
    /// Parameter bindings the worker is locked to (AD-036).
    #[serde(default)]
    pub bound_parameters: Vec<WorkerBoundParameter>,
    /// Worker grant expires at or before this instant (narrowing only).
    pub expires_before: jiff::Timestamp,
    /// Purpose slug for the task.
    pub purpose: String,
    /// Route/workflow/pack lineage copied from the master for audit.
    pub route_id: String,
    pub workflow_id: String,
    pub capability_pack_id: String,
    /// Optional counterparty the worker operates on (D-085 briefcase
    /// scoping). `None` means a generic/owner task with no external
    /// counterparty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counterparty_channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counterparty_identifier: Option<String>,
    /// D-085: the dispatch lane derives the briefcase task class.
    #[serde(default)]
    pub task_class: crate::briefcase::TaskClass,
}

/// One AD-036 bound parameter in a commissioning spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkerBoundParameter {
    pub name: String,
    pub value: String,
}

impl WorkerResult {
    /// Build a minimal `Completed` result with no slots/requests/notes.
    pub fn completed() -> Self {
        WorkerResult {
            outcome: WorkerOutcome::Completed,
            offered_slots: vec![],
            requests: vec![],
            notes_ref: None,
        }
    }
}
