//! Exact active-version lookup shared by overlay admission and review.
use crate::artifact_loader::ArtifactRegistry;
use openspine_schemas::artifact::Lifecycle;

pub(crate) fn registry_entry_active_at(
    registry: &ArtifactRegistry,
    kind: &str,
    id: &str,
    version: u32,
) -> bool {
    match kind {
        "route" => registry
            .routes
            .iter()
            .any(|r| r.id == id && r.version == version && r.lifecycle_state == Lifecycle::Active),
        "agent" => registry
            .agents
            .get(id)
            .is_some_and(|a| a.version == version && a.lifecycle_state == Lifecycle::Active),
        "workflow" => registry
            .workflows
            .get(id)
            .is_some_and(|w| w.version == version && w.lifecycle_state == Lifecycle::Active),
        "pack" => registry
            .packs
            .get(id)
            .is_some_and(|p| p.version == version && p.lifecycle_state == Lifecycle::Active),
        "policy" => registry
            .policies
            .get(id)
            .is_some_and(|p| p.version == version && p.lifecycle_state == Lifecycle::Active),
        "template" => registry
            .templates
            .get(id)
            .is_some_and(|t| t.version == version && t.lifecycle_state == Lifecycle::Active),
        "model_swap" => registry
            .model_swaps
            .get(id)
            .is_some_and(|m| m.version == version && m.lifecycle_state == Lifecycle::Active),
        "standing_rule" => registry
            .standing_rules
            .get(id)
            .is_some_and(|r| r.version == version && r.lifecycle_state == Lifecycle::Active),
        "persona" => registry
            .personas
            .get(id)
            .is_some_and(|p| p.version == version && p.lifecycle_state == Lifecycle::Active),
        _ => false,
    }
}
