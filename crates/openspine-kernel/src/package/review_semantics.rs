//! Deterministic typed runtime-input comparison. No permission interpretation.
use std::collections::{BTreeMap, BTreeSet};

use openspine_schemas::action::ActionId;
use openspine_schemas::digest::{digest_of, digest_of_bytes, Digest};
use serde::Serialize;
use serde_json::{json, Value};

use crate::artifact_loader::ArtifactRegistry;

pub(crate) const MAX_DETAIL_BYTES: usize = 64 * 1024;

#[derive(Debug, Serialize)]
pub(crate) struct SemanticReview {
    pub before_runtime_digest: Digest,
    pub after_runtime_digest: Digest,
    pub changes: Vec<ArtifactChange>,
    /// Unchanged typed artifacts make references in changed routes/workflows
    /// inspectable without reopening a source or inventing a semantic verdict.
    pub supporting_artifacts: Vec<Value>,
    pub source_changes: Vec<Value>,
    pub action_descriptors: Value,
    pub blockers: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ArtifactChange {
    pub kind: String,
    pub id: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
}

type Inputs = BTreeMap<(String, String), Value>;

pub(crate) struct InvalidRuntimeArtifact {
    pub kind: &'static str,
    pub id: String,
    pub version: u32,
    pub code: &'static str,
}

/// Reuse typed validation for both captured bases and admitted overlays.
/// Findings contain fixed codes; parser text and artifact labels are not verdicts.
pub(crate) fn validation_findings(registry: &ArtifactRegistry) -> Vec<InvalidRuntimeArtifact> {
    let mut findings = Vec::new();
    for workflow in registry.workflows.values() {
        if workflow.validate().is_err() {
            findings.push(InvalidRuntimeArtifact {
                kind: "workflow",
                id: workflow.id.clone(),
                version: workflow.version,
                code: "invalid-workflow-semantics",
            });
        }
    }
    for route in &registry.routes {
        if route
            .when
            .actor
            .as_ref()
            .and_then(|actor| actor.identity_confidence_min)
            .is_some_and(|value| !value.is_finite())
        {
            // serde_json represents non-finite floats as null, losing meaning.
            findings.push(InvalidRuntimeArtifact {
                kind: "route",
                id: route.id.clone(),
                version: route.version,
                code: "unrenderable-typed-value",
            });
        }
    }
    findings.sort_by(|a, b| (a.kind, &a.id, a.version).cmp(&(b.kind, &b.id, b.version)));
    findings
}

pub(crate) fn compare(before: &ArtifactRegistry, after: &ArtifactRegistry) -> SemanticReview {
    let old = typed_inputs(before);
    let new = typed_inputs(after);
    let mut blockers = BTreeSet::new();
    for registry in [before, after] {
        for finding in validation_findings(registry) {
            blockers.insert(finding.code.into());
        }
    }
    let mut changes = Vec::new();
    let mut supporting_artifacts = Vec::new();
    for key in old.keys().chain(new.keys()).collect::<BTreeSet<_>>() {
        let a = old.get(key);
        let b = new.get(key);
        if a == b {
            supporting_artifacts.push(bounded_detail(a.unwrap(), &mut blockers));
        } else {
            changes.push(ArtifactChange {
                kind: key.0.clone(),
                id: key.1.clone(),
                before: a.map(|value| bounded_detail(value, &mut blockers)),
                after: b.map(|value| bounded_detail(value, &mut blockers)),
            });
        }
    }
    let actions = actions(before).into_iter().chain(actions(after)).collect();
    let action_descriptors = describe_actions(actions, &mut blockers);
    SemanticReview {
        before_runtime_digest: runtime_digest(&old),
        after_runtime_digest: runtime_digest(&new),
        changes,
        supporting_artifacts,
        source_changes: source_changes(before, after),
        action_descriptors,
        blockers: blockers.into_iter().collect(),
    }
}

fn typed_inputs(registry: &ArtifactRegistry) -> Inputs {
    let mut values = BTreeMap::new();
    macro_rules! add {
        ($kind:literal, $items:expr) => {
            for artifact in $items {
                let fields = serde_json::to_value(artifact).expect("typed artifact serializes");
                let version = fields.get("version").and_then(Value::as_u64);
                let key = ($kind.to_string(), artifact.id.clone());
                let source = registry.sources.get(&(
                    key.0.clone(), key.1.clone(), version.unwrap_or(1) as u32,
                ));
                values.insert(key, json!({
                    "kind": $kind, "id": artifact.id, "version": version,
                    "source_digest": source.map(|source| digest_of_bytes(&source.bytes)),
                    "fields": fields,
                }));
            }
        };
    }
    add!("route", &registry.routes);
    add!("agent", registry.agents.values());
    add!("workflow", registry.workflows.values());
    add!("pack", registry.packs.values());
    add!("policy", registry.policies.values());
    add!("template", registry.templates.values());
    add!("golden_set", registry.golden_sets.values());
    add!("model_swap", registry.model_swaps.values());
    add!("standing_rule", registry.standing_rules.values());
    add!("persona", registry.personas.values());
    values
}

fn runtime_digest(inputs: &Inputs) -> Digest {
    // Raw bytes are bound separately by source digests and package identities.
    // Typed fields determine this base runtime-input digest. Equal digests do
    // not establish authority equivalence or make a formatting edit approved.
    let fields: Vec<_> = inputs
        .values()
        .map(|value| {
            json!({
                "kind": value["kind"], "id": value["id"], "fields": value["fields"],
            })
        })
        .collect();
    digest_of(&json!({"schema_version": 1, "typed_base_inputs": fields}))
}

fn bounded_detail(value: &Value, blockers: &mut BTreeSet<String>) -> Value {
    if super::review_report::escaped_json(value, true).len() <= MAX_DETAIL_BYTES {
        return value.clone();
    }
    blockers.insert("semantic-detail-limit-exceeded".into());
    json!({
        "kind": value["kind"], "id": value["id"], "version": value["version"],
        "source_digest": value["source_digest"], "detail_digest": digest_of(value),
        "detail_status": "limit-exceeded", "required_detail_rendered": false,
    })
}

fn source_changes(before: &ArtifactRegistry, after: &ArtifactRegistry) -> Vec<Value> {
    let keys: BTreeSet<_> = before.sources.keys().chain(after.sources.keys()).collect();
    keys.into_iter()
        .filter_map(|(kind, id, version)| {
            let key = (kind.clone(), id.clone(), *version);
            let a = before.sources.get(&key).map(|s| digest_of_bytes(&s.bytes));
            let b = after.sources.get(&key).map(|s| digest_of_bytes(&s.bytes));
            (a != b).then(|| {
                json!({
                    "kind": kind, "id": id,
                    "version": (kind != "golden_set").then_some(version),
                    "before_digest": a, "after_digest": b,
                })
            })
        })
        .collect()
}

fn actions(registry: &ArtifactRegistry) -> BTreeSet<ActionId> {
    let mut actions = BTreeSet::new();
    for agent in registry.agents.values() {
        actions.extend(agent.designed_tools.iter().cloned());
        actions.extend(agent.approval_required_tools.iter().cloned());
        actions.extend(agent.denied_tools.iter().cloned());
    }
    macro_rules! lists {
        ($items:expr) => {
            for artifact in $items {
                actions.extend(artifact.candidate_allowed_actions.iter().cloned());
                actions.extend(artifact.approval_required.iter().cloned());
                actions.extend(artifact.denied_actions.iter().cloned());
            }
        };
    }
    lists!(registry.packs.values());
    lists!(registry.policies.values());
    lists!(registry.workflows.values());
    for workflow in registry.workflows.values() {
        actions.extend(
            workflow
                .states
                .iter()
                .filter_map(|s| s.approval_action.clone()),
        );
    }
    actions.extend(
        registry
            .standing_rules
            .values()
            .map(|r| r.action_id.clone()),
    );
    actions
}

fn describe_actions(actions: BTreeSet<ActionId>, blockers: &mut BTreeSet<String>) -> Value {
    let catalog = crate::action_catalog::canonical_catalog();
    let mut descriptions = BTreeMap::new();
    for action in actions {
        let descriptor = catalog.tool_descriptor_for(&action);
        if !catalog.contains(&action) {
            blockers.insert("unknown-catalog-action".into());
        }
        if descriptor.is_none() && catalog.delegation_descriptor_for(&action).is_none() {
            blockers.insert("missing-owner-action-description".into());
        }
        if catalog.egress_decl_for(&action).is_none() {
            blockers.insert("missing-action-egress-declaration".into());
        }
        descriptions.insert(
            action.as_str().to_owned(),
            json!({
                "source": "kernel-action-catalog",
                "known": catalog.contains(&action),
                "tool": descriptor,
                "non_effect_stub": catalog.is_non_effect_stub(&action),
                "kernel_origin": catalog.is_kernel_origin(&action),
                "non_delegable": catalog.is_non_delegable(&action),
                "egress_class": catalog.egress_class_for(&action),
                "output_channels": catalog.output_channel_for(&action),
                "delegation": catalog.delegation_descriptor_for(&action),
                "implementation": catalog.implementation_descriptor_for_action(&action),
            }),
        );
    }
    serde_json::to_value(descriptions).expect("catalog descriptions serialize")
}

#[cfg(test)]
#[path = "review_semantics_tests.rs"]
mod tests;
