// openspine:allow-large-module reason: overlay compatibility checks (dangling reference detection, version resolution)
#[path = "overlay_convergence.rs"]
mod overlay_convergence;
pub use overlay_convergence::converge_owner_accepted_dependencies;

use std::collections::HashSet;

use openspine_schemas::artifact::Lifecycle;

use crate::artifact_loader::ArtifactRegistry;
use crate::store::learned_artifacts::{CompatibilityStatus, LearnedArtifact};
use ulid::Ulid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanedArtifact {
    pub kind: String,
    pub artifact_id: String,
    pub version: u32,
    pub dangling_references: Vec<String>,
}
pub fn compatibility_epoch(
    registry: &ArtifactRegistry,
    base_ids: &std::collections::HashSet<(String, String)>,
) -> String {
    let mut entries = Vec::new();
    macro_rules! add_map {
        ($kind:literal, $map:expr) => {
            for artifact in $map.values() {
                if artifact.lifecycle_state != Lifecycle::Active
                    || !base_ids.contains(&($kind.into(), artifact.id.clone()))
                {
                    continue;
                }
                let digest = registry
                    .sources
                    .get(&($kind.into(), artifact.id.clone(), artifact.version))
                    .map(|source| {
                        openspine_schemas::digest::digest_of_bytes(&source.bytes).to_string()
                    })
                    .unwrap_or_default();
                entries.push(format!(
                    "{}|{}|{}|{}",
                    $kind, artifact.id, artifact.version, digest
                ));
            }
        };
    }
    for route in &registry.routes {
        if route.lifecycle_state == Lifecycle::Active
            && base_ids.contains(&("route".into(), route.id.clone()))
        {
            let digest = registry
                .sources
                .get(&("route".into(), route.id.clone(), route.version))
                .map(|source| openspine_schemas::digest::digest_of_bytes(&source.bytes).to_string())
                .unwrap_or_default();
            entries.push(format!("route|{}|{}|{digest}", route.id, route.version));
        }
    }
    add_map!("agent", registry.agents);
    add_map!("workflow", registry.workflows);
    add_map!("pack", registry.packs);
    add_map!("policy", registry.policies);
    add_map!("template", registry.templates);
    entries.sort();
    openspine_schemas::digest::digest_of(&serde_json::json!(entries)).to_string()
}

/// Find dangling references in a parsed overlay proposal.
pub fn dangling_for_parsed(
    registry: &ArtifactRegistry,
    parsed: &crate::artifact_loader::ParsedProposal,
) -> Vec<String> {
    let agents: HashSet<&str> = registry
        .agents
        .values()
        .filter(|a| a.lifecycle_state == Lifecycle::Active)
        .map(|a| a.id.as_str())
        .collect();
    let workflows: HashSet<&str> = registry
        .workflows
        .values()
        .filter(|a| a.lifecycle_state == Lifecycle::Active)
        .map(|a| a.id.as_str())
        .collect();
    let packs: HashSet<&str> = registry
        .packs
        .values()
        .filter(|a| a.lifecycle_state == Lifecycle::Active)
        .map(|a| a.id.as_str())
        .collect();
    match parsed {
        crate::artifact_loader::ParsedProposal::Route(route) => {
            let mut out = Vec::new();
            if let Some(id) = route.agent.as_deref() {
                if !agents.contains(id) {
                    out.push(format!("agent:{id}"));
                }
            }
            if let Some(id) = route.workflow.as_deref() {
                if !workflows.contains(id) {
                    out.push(format!("workflow:{id}"));
                }
            }
            if let Some(id) = route.capability_pack.as_deref() {
                if !packs.contains(id) {
                    out.push(format!("pack:{id}"));
                }
            }
            out
        }
        crate::artifact_loader::ParsedProposal::Workflow(workflow) => {
            let mut out = Vec::new();
            if !agents.contains(workflow.required_agent.as_str()) {
                out.push(format!("agent:{}", workflow.required_agent));
            }
            if !packs.contains(workflow.required_capability_pack.as_str()) {
                out.push(format!("pack:{}", workflow.required_capability_pack));
            }
            out
        }
        _ => Vec::new(),
    }
}

pub fn owner_accepted_newly_dangling(
    registry: &ArtifactRegistry,
    kind: &str,
    source_bytes: Option<&[u8]>,
) -> Vec<String> {
    let Some(bytes) = source_bytes else {
        return vec!["owner_accepted_source_missing".into()];
    };
    // PromptTemplates carry no typed dependency references, so a reconfirmed
    // legacy template is trusted (no deps) and must not be excluded or
    // re-prompted every restart. Only an unparseable template is invalid.
    if kind == "template" {
        return match serde_yaml::from_slice::<crate::model_gateway::PromptTemplate>(bytes) {
            Ok(_) => Vec::new(),
            Err(_) => vec!["owner_accepted_template_parse_failed".into()],
        };
    }
    let yaml = String::from_utf8_lossy(bytes);
    match crate::artifact_loader::parse_proposal(kind, &yaml) {
        Ok(parsed) => dangling_for_parsed(registry, &parsed),
        Err(_) => vec!["owner_accepted_parse_failed".into()],
    }
}
/// bump: unrelated learned state survives an update without needless prompts.
pub fn find_orphans(
    registry: &ArtifactRegistry,
    learned: &[LearnedArtifact],
) -> Vec<OrphanedArtifact> {
    let agents: HashSet<&str> = registry
        .agents
        .values()
        .filter(|artifact| artifact.lifecycle_state == Lifecycle::Active)
        .map(|artifact| artifact.id.as_str())
        .collect();
    let workflows: HashSet<&str> = registry
        .workflows
        .values()
        .filter(|artifact| artifact.lifecycle_state == Lifecycle::Active)
        .map(|artifact| artifact.id.as_str())
        .collect();
    let packs: HashSet<&str> = registry
        .packs
        .values()
        .filter(|artifact| artifact.lifecycle_state == Lifecycle::Active)
        .map(|artifact| artifact.id.as_str())
        .collect();
    let mut out = Vec::new();

    for item in learned {
        if item.namespace != openspine_schemas::artifact::ArtifactNamespace::Overlay {
            continue;
        }
        let mut dangling = Vec::new();
        match item.kind.as_str() {
            "route" => {
                if let Some(route) = registry
                    .routes
                    .iter()
                    .find(|route| route.id == item.artifact_id && route.version == item.version)
                {
                    if route.lifecycle_state != Lifecycle::Active {
                        continue;
                    }
                    if let Some(id) = route.agent.as_deref() {
                        if !agents.contains(id) {
                            dangling.push(format!("agent:{id}"));
                        }
                    }
                    if let Some(id) = route.workflow.as_deref() {
                        if !workflows.contains(id) {
                            dangling.push(format!("workflow:{id}"));
                        }
                    }
                    if let Some(id) = route.capability_pack.as_deref() {
                        if !packs.contains(id) {
                            dangling.push(format!("pack:{id}"));
                        }
                    }
                }
            }
            "workflow" => {
                if let Some(workflow) = registry.workflows.get(&item.artifact_id) {
                    if workflow.version != item.version
                        || workflow.lifecycle_state != Lifecycle::Active
                    {
                        continue;
                    }
                    if !agents.contains(workflow.required_agent.as_str()) {
                        dangling.push(format!("agent:{}", workflow.required_agent));
                    }
                    if !packs.contains(workflow.required_capability_pack.as_str()) {
                        dangling.push(format!("pack:{}", workflow.required_capability_pack));
                    }
                }
            }
            _ => {}
        }
        if !dangling.is_empty() {
            out.push(OrphanedArtifact {
                kind: item.kind.clone(),
                artifact_id: item.artifact_id.clone(),
                version: item.version,
                dangling_references: dangling,
            });
        }
    }
    out
}

pub fn exclude_orphans(registry: &mut ArtifactRegistry, orphans: &[OrphanedArtifact]) {
    registry.routes.retain(|route| {
        !orphans.iter().any(|orphan| {
            orphan.kind == "route"
                && orphan.artifact_id == route.id
                && orphan.version == route.version
        })
    });
    for orphan in orphans.iter().filter(|orphan| orphan.kind == "workflow") {
        if registry
            .workflows
            .get(&orphan.artifact_id)
            .is_some_and(|w| w.version == orphan.version)
        {
            registry.workflows.remove(&orphan.artifact_id);
        }
    }
    for orphan in orphans.iter().filter(|orphan| orphan.kind == "agent") {
        if registry
            .agents
            .get(&orphan.artifact_id)
            .is_some_and(|a| a.version == orphan.version)
        {
            registry.agents.remove(&orphan.artifact_id);
        }
    }
    for orphan in orphans.iter().filter(|orphan| orphan.kind == "pack") {
        if registry
            .packs
            .get(&orphan.artifact_id)
            .is_some_and(|p| p.version == orphan.version)
        {
            registry.packs.remove(&orphan.artifact_id);
        }
    }
    for orphan in orphans.iter().filter(|orphan| orphan.kind == "policy") {
        if registry
            .policies
            .get(&orphan.artifact_id)
            .is_some_and(|p| p.version == orphan.version)
        {
            registry.policies.remove(&orphan.artifact_id);
        }
    }
    for orphan in orphans.iter().filter(|orphan| orphan.kind == "template") {
        if registry
            .templates
            .get(&orphan.artifact_id)
            .is_some_and(|t| t.version == orphan.version)
        {
            registry.templates.remove(&orphan.artifact_id);
        }
    }
}

fn registry_entry_active_at(
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
            .is_some_and(|t| t.version == version),
        _ => false,
    }
}

pub fn missing_provenance(
    overlay: &ArtifactRegistry,
    learned: &[LearnedArtifact],
) -> Vec<OrphanedArtifact> {
    let mut keys = Vec::new();
    keys.extend(
        overlay
            .routes
            .iter()
            .map(|a| ("route".into(), a.id.clone(), a.version)),
    );
    keys.extend(
        overlay
            .agents
            .values()
            .map(|a| ("agent".into(), a.id.clone(), a.version)),
    );
    keys.extend(
        overlay
            .workflows
            .values()
            .map(|a| ("workflow".into(), a.id.clone(), a.version)),
    );
    keys.extend(
        overlay
            .packs
            .values()
            .map(|a| ("pack".into(), a.id.clone(), a.version)),
    );
    keys.extend(
        overlay
            .policies
            .values()
            .map(|a| ("policy".into(), a.id.clone(), a.version)),
    );
    keys.extend(
        overlay
            .templates
            .values()
            .map(|a| ("template".into(), a.id.clone(), a.version)),
    );
    keys.into_iter()
        .filter(|(kind, id, version)| {
            !learned.iter().any(|item| {
                &item.kind == kind && &item.artifact_id == id && item.version == *version
            })
        })
        .map(|(kind, id, version)| OrphanedArtifact {
            kind,
            artifact_id: id,
            version,
            dangling_references: vec!["missing_provenance".into()],
        })
        .collect()
}

pub fn apply_compatibility(
    registry: &mut ArtifactRegistry,
    learned: &[LearnedArtifact],
) -> (Vec<OrphanedArtifact>, Vec<Ulid>) {
    let mut all = Vec::new();
    let pending: Vec<_> = learned
        .iter()
        .filter(|item| {
            item.compatibility == CompatibilityStatus::ReconfirmationRequired
                && registry_entry_active_at(registry, &item.kind, &item.artifact_id, item.version)
        })
        .map(|item| OrphanedArtifact {
            kind: item.kind.clone(),
            artifact_id: item.artifact_id.clone(),
            version: item.version,
            dangling_references: vec!["reconfirmation_required".into()],
        })
        .collect();
    exclude_orphans(registry, &pending);
    all.extend(pending);
    loop {
        let next = find_orphans(registry, learned)
            .into_iter()
            .filter(|candidate| {
                // Never re-orphan a durably owner-accepted artifact; the owner's
                // single tap endures even with dangling references (AD-070).
                let owner_accepted = learned.iter().any(|item| {
                    item.kind == candidate.kind
                        && item.artifact_id == candidate.artifact_id
                        && item.version == candidate.version
                        && item.compatibility == CompatibilityStatus::OwnerAccepted
                });
                if owner_accepted {
                    return false;
                }
                // Version cutover: a stale learned row for a superseded version
                // must not exclude the active higher version.
                if !registry_entry_active_at(
                    registry,
                    &candidate.kind,
                    &candidate.artifact_id,
                    candidate.version,
                ) {
                    return false;
                }
                !all.iter().any(|existing| {
                    existing.kind == candidate.kind
                        && existing.artifact_id == candidate.artifact_id
                        && existing.version == candidate.version
                })
            })
            .collect::<Vec<_>>();
        if next.is_empty() {
            break;
        }
        exclude_orphans(registry, &next);
        all.extend(next);
    }
    let requests = all
        .iter()
        .map(|item| {
            learned
                .iter()
                .find(|candidate| {
                    candidate.kind == item.kind
                        && candidate.artifact_id == item.artifact_id
                        && candidate.version == item.version
                })
                .and_then(|candidate| candidate.pending_reconfirmation_id)
                .unwrap_or_else(Ulid::new)
        })
        .collect();
    (all, requests)
}
/// cannot silently point at stale bytes.
pub fn ensure_reconfirm_request(
    store: &crate::store::Store,
    kind: &str,
    artifact_id: &str,
    version: u32,
    request_id: Ulid,
    review_ref: openspine_schemas::artifact::ArtifactRef,
) -> Result<Ulid, crate::store::StoreError> {
    let target_digest = openspine_schemas::digest::digest_of(&serde_json::json!({
        "kind": kind,
        "artifact_id": artifact_id,
        "version": version,
    }));
    let chosen_id = match store.find_action_request(request_id)? {
        Some(existing)
            if !store.is_action_request_used(request_id)?
                && existing.action.as_str() == "artifact.reconfirm"
                && existing.payload_ref.as_ref() == Some(&review_ref)
                && existing.target_digest.as_ref() == Some(&target_digest) =>
        {
            request_id
        }
        Some(_) => Ulid::new(),
        None => request_id,
    };
    if chosen_id == request_id && store.find_action_request(chosen_id)?.is_some() {
        return Ok(chosen_id);
    }
    let request = openspine_schemas::action::ActionRequest {
        id: chosen_id,
        task_grant_id: Ulid::new(),
        action: openspine_schemas::action::ActionId::new("artifact.reconfirm"),
        target_ref: None,
        payload_ref: Some(review_ref),
        target_digest: Some(target_digest),
        selection_token_id: None,
        params: std::collections::BTreeMap::new(),
        skill_attribution: None,
        requested_at: jiff::Timestamp::now(),
        schema_version: 1,
    };
    store.insert_action_request(&request)?;
    Ok(chosen_id)
}

#[cfg(test)]
#[path = "overlay_compat_tests.rs"]
mod tests;
