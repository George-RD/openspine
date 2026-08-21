//! Kernel-owned per-action catalog metadata value types, split from
//! `action.rs` to keep it under the 500-line module cap. Both types are
//! *declared* per action and owned by the kernel `ActionCatalog`, never
//! derived from optional connector metadata and never carried on a
//! `TaskGrant`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{ActionCatalog, ActionId};
use crate::egress::EgressClass;

/// The catalog-owned egress metadata for one registered action (blocker 1).
///
/// Both axes are *declared* per action and owned by the kernel catalog, never
/// derived from optional connector metadata. `None` on an axis means the
/// action carries no requirement on that axis (e.g. a non-egress action has
/// `egress_class: None`; a non-output action has `output_channels: None`).
/// An empty `Some(vec![])` on `output_channels` is a deliberate,
/// fail-closed declaration (the action is classified as delivering to a
/// channel but names none — the gate must deny).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActionEgressDeclaration {
    pub output_channels: Option<Vec<String>>,
    pub egress_class: Option<EgressClass>,
}

/// The catalog-owned, model-facing tool descriptor for one dispatchable action
/// (spec #209, the pre-inference "capability-derived tool catalog" lane). This
/// is the presentation surface a worker projects into its model's tool schema;
/// it is kernel-owned catalog metadata, never shell-spoofable and never carried
/// on a `TaskGrant`. The projection function (IT2) and wire seam (IT3) consume
/// this axis; this ticket only defines and populates it.
///
/// The descriptor carries no `action_id` field — the catalog map key carries
/// the id, exactly as [`ActionEgressDeclaration`] does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDescriptor {
    /// The LLM-facing tool name presented to the model.
    pub name: String,
    /// A one-line, model-facing description of what the tool does.
    pub description: String,
    /// A JSON Schema for the action's invocation payload. Free-form JSON so
    /// the crate's established embedded-schema convention (`serde_json::Value`)
    /// applies; this is a projection/presentation surface, not a dispatch-time
    /// validation layer.
    pub parameters_schema: Value,
    /// Presentation flag: the tool is proposable but its effect pauses for
    /// owner approval via the existing gate/approval flow.
    pub approval_required: bool,
    /// Presentation flag: invoking the tool requires a valid, grant-bound
    /// selection token. Mirrors `ActionCatalog::requires_selection_token`.
    pub selection_token_required: bool,
}

/// `parameters_schema` is a `serde_json::Value`, which is only `PartialEq`;
/// this manual `impl Eq` lets `ToolDescriptor` be a value in `ActionCatalog`'s
/// derived `Eq` without changing any other type.
impl Eq for ToolDescriptor {}

impl ActionCatalog {
    /// Declare the catalog-owned tool descriptors for a set of actions (spec
    /// #209). Assigns (overwrites) the map, mirroring `with_egress_declarations`
    /// exactly. The kernel populates one descriptor per dispatchable action; a
    /// dispatchable action lacking one is caught by a fail-closed completeness
    /// test in the gate.
    pub fn with_tool_descriptors(
        mut self,
        descriptors: impl IntoIterator<Item = (ActionId, ToolDescriptor)>,
    ) -> Self {
        self.tool_descriptors = descriptors.into_iter().collect();
        self
    }

    /// If `id` carries a tool descriptor, returns it; `None` otherwise.
    /// Mirrors `egress_decl_for`.
    pub fn tool_descriptor_for(&self, id: &ActionId) -> Option<&ToolDescriptor> {
        self.tool_descriptors.get(id)
    }

    /// How many tool descriptors the catalog holds. Exposed so a completeness
    /// test can pin the dispatchable descriptor count by cardinality, not only
    /// by membership (mirrors `non_effect_stub_count`).
    pub fn tool_descriptor_count(&self) -> usize {
        self.tool_descriptors.len()
    }
}
