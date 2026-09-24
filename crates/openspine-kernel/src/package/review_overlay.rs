//! Pure overlay consequences for a captured base transition. No activation.
use super::current_state::{ArtifactVersion, CapturedCurrentState, CapturedOverlayState};
use crate::artifact_loader::{self, ArtifactRegistry, ParsedProposal};
use crate::overlay_compat::{self, overlay_admission, overlay_convergence, OrphanedArtifact};
use crate::store::learned_artifacts::CompatibilityStatus;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::digest::{digest_of, digest_of_bytes};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[path = "review_overlay_fixtures.rs"]
mod fixtures;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) struct OverlayConsequence {
    pub kind: String,
    pub artifact_id: String,
    pub version: u32,
    pub reason: String,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct OverlayViewAssessment {
    /// Effective overlay identities only; base artifacts are described separately.
    pub effective_artifacts: Vec<ArtifactVersion>,
    /// Excluded overlays and unresolved base route/workflow references.
    pub consequences: Vec<OverlayConsequence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct OverlayAssessment {
    pub current_base_epoch: String,
    pub candidate_base_epoch: String,
    /// Binds overlay sources, learned provenance and captured proposal controls.
    /// Future acceptance must separately bind live reusable-authority controls;
    /// the outstanding-work census is not a full authority-state fingerprint.
    pub control_fingerprint: String,
    pub before: OverlayViewAssessment,
    pub after: OverlayViewAssessment,
    pub blockers: Vec<OverlayConsequence>,
    pub ignored_fixtures: Vec<fixtures::IgnoredOverlayFixture>,
    /// Persona files not included by the provenance-gated loader. These are
    /// relative paths, not artifact IDs; the inventory still binds their bytes.
    pub ignored_persona_files: Vec<String>,
    pub reusable_authority_reconfirmation_required: bool,
}

pub(crate) fn assess(
    current: &CapturedCurrentState,
    candidate: &ArtifactRegistry,
) -> anyhow::Result<OverlayAssessment> {
    let current_ids = artifact_loader::artifact_identity_pairs(&current.base.registry);
    let current_epoch = overlay_compat::compatibility_epoch(&current.base.registry, &current_ids);
    anyhow::ensure!(
        current_ids == current.base_artifact_ids
            && current_epoch == current.base_compatibility_epoch,
        "captured base identity disagrees"
    );
    let candidate_ids = artifact_loader::artifact_identity_pairs(candidate);
    let candidate_epoch = overlay_compat::compatibility_epoch(candidate, &candidate_ids);
    let epoch_changed = current_epoch != candidate_epoch;
    let ignored_fixtures = fixtures::assess(&current.overlay)?;
    let mut keys = BTreeSet::new();
    for row in &current.overlay.learned {
        let key = (row.kind.clone(), row.artifact_id.clone(), row.version);
        anyhow::ensure!(
            keys.insert(key.clone()),
            "duplicate captured learned provenance"
        );
        anyhow::ensure!(
            current.overlay.controls.contains_key(&key),
            "captured learned control missing"
        );
    }
    for ((kind, id, version), control) in &current.overlay.controls {
        if kind == "golden_set" {
            continue; // Fixture controls were validated separately above.
        }
        anyhow::ensure!(
            matches!(
                kind.as_str(),
                "route"
                    | "agent"
                    | "workflow"
                    | "pack"
                    | "policy"
                    | "template"
                    | "model_swap"
                    | "standing_rule"
                    | "persona"
            ) && *version > 0,
            "unsupported captured overlay identity"
        );
        if kind != "persona" {
            if let Some(highest) = control.highest_active_version {
                anyhow::ensure!(
                    current
                        .overlay
                        .controls
                        .contains_key(&(kind.clone(), id.clone(), highest)),
                    "captured highest-version control missing"
                );
            }
        }
    }
    let control_fingerprint = fingerprint(&current.overlay);
    let (before, mut blockers) = view(&current.overlay, &current.base.registry, false)?;
    let (after, candidate_blockers) = view(&current.overlay, candidate, epoch_changed)?;
    blockers.extend(candidate_blockers);
    canonicalize(&mut blockers);
    let mut ignored_persona_files = current.overlay.ignored_persona_files.clone();
    ignored_persona_files.sort();
    Ok(OverlayAssessment {
        current_base_epoch: current_epoch,
        candidate_base_epoch: candidate_epoch,
        control_fingerprint,
        before,
        after,
        blockers,
        ignored_fixtures,
        ignored_persona_files,
        reusable_authority_reconfirmation_required: epoch_changed,
    })
}

fn view(
    captured: &CapturedOverlayState,
    base: &ArtifactRegistry,
    epoch_changed: bool,
) -> anyhow::Result<(OverlayViewAssessment, Vec<OverlayConsequence>)> {
    let admitted = captured.evaluate_versions()?;
    let mut overlay = admitted.registry;
    let base_ids = artifact_loader::artifact_identity_pairs(base);
    let mut consequences: Vec<_> = admitted
        .excluded
        .iter()
        .map(|key| consequence(key, "inactive_or_superseded", vec![]))
        .collect();
    for key in overlay.sources.keys().filter(|key| key.0 != "golden_set") {
        if artifact_loader::artifact_version(&overlay, &key.0, &key.1) != Some(key.2) {
            consequences.push(consequence(key, "inactive_or_superseded", vec![]));
        } else if !overlay_compat::registry_entry_active_at(&overlay, &key.0, &key.1, key.2) {
            consequences.push(consequence(key, "inactive_lifecycle", vec![]));
        }
    }
    let mut blockers = Vec::new();
    for finding in super::review_semantics::validation_findings(&overlay) {
        blockers.push(consequence(
            &(finding.kind.into(), finding.id, finding.version),
            finding.code,
            vec![],
        ));
    }
    let rows: BTreeMap<_, _> = captured
        .learned
        .iter()
        .map(|row| {
            (
                (row.kind.clone(), row.artifact_id.clone(), row.version),
                row,
            )
        })
        .collect();
    for (key, control) in &captured.controls {
        let row = rows.get(key);
        if row.is_some_and(|row| row.compatibility == CompatibilityStatus::Erased) {
            consequences.push(consequence(key, "erased", vec![]));
            continue;
        }
        if matches!(key.0.as_str(), "persona" | "golden_set") {
            continue;
        }
        if control.lifecycle == Some(Lifecycle::Active)
            && control
                .highest_active_version
                .is_none_or(|highest| highest < key.2)
        {
            blockers.push(consequence(key, "active_version_control_disagrees", vec![]));
        }
        if control.highest_active_version == Some(key.2) {
            if control.lifecycle != Some(Lifecycle::Active) {
                blockers.push(consequence(
                    key,
                    "highest_active_lifecycle_disagrees",
                    vec![],
                ));
            }
            if !control.source_present {
                blockers.push(consequence(
                    key,
                    "highest_active_source_missing",
                    vec![if control.recoverable_blob_present {
                        "recovery_required"
                    } else {
                        "recovery_bytes_unavailable"
                    }
                    .into()],
                ));
            }
            if !rows.contains_key(key) {
                blockers.push(consequence(key, "missing_provenance", vec![]));
            }
            if overlay.sources.contains_key(key)
                && !overlay_compat::registry_entry_active_at(&overlay, &key.0, &key.1, key.2)
            {
                blockers.push(consequence(
                    key,
                    "active_source_lifecycle_disagrees",
                    vec![],
                ));
            }
        }
    }
    overlay_compat::exclude_erased(&mut overlay, &captured.learned);
    for ((id, version), reason) in &captured.persona_findings.excluded {
        let reason = match reason {
            crate::overlay_persona_admission::PersonaProvenanceExclusion::Erased => "erased",
            crate::overlay_persona_admission::PersonaProvenanceExclusion::NotProducedBy => {
                "persona_not_produced_by"
            }
            crate::overlay_persona_admission::PersonaProvenanceExclusion::EventUnavailable => {
                "persona_event_unavailable"
            }
            crate::overlay_persona_admission::PersonaProvenanceExclusion::ExchangeNotBound => {
                "persona_exchange_not_bound"
            }
            crate::overlay_persona_admission::PersonaProvenanceExclusion::ExchangeUnavailable => {
                "persona_exchange_unavailable"
            }
            crate::overlay_persona_admission::PersonaProvenanceExclusion::DigestMissing => {
                "persona_digest_missing"
            }
        };
        consequences.push(consequence(
            &("persona".into(), id.clone(), *version),
            reason,
            vec![],
        ));
    }
    for ((id, version), digest) in &captured.persona_findings.expected_digests {
        let key = ("persona".into(), id.clone(), *version);
        if captured
            .registry
            .sources
            .get(&key)
            .is_none_or(|source| digest_of_bytes(&source.bytes).as_str() != digest)
        {
            blockers.push(consequence(
                &key,
                "persona_source_missing_or_invalid",
                vec![],
            ));
        }
    }
    // Defence in depth for manually assembled/damaged captures: a persona can
    // participate only with exactly the captured provenance-approved digest.
    for (key, source) in &overlay.sources {
        if key.0 == "persona"
            && captured
                .persona_findings
                .expected_digests
                .get(&(key.1.clone(), key.2))
                .map(String::as_str)
                != Some(digest_of_bytes(&source.bytes).as_str())
        {
            blockers.push(consequence(key, "persona_provenance_unavailable", vec![]));
        }
    }
    let admission = overlay_admission::evaluate(&overlay, &captured.learned, &base_ids);
    for (kind, id) in &admission.collisions {
        if let Some(version) = artifact_loader::artifact_version(&overlay, kind, id) {
            consequences.push(consequence(
                &(kind.clone(), id.clone(), version),
                "base_overlay_collision",
                vec![],
            ));
        }
    }
    for item in &admission.digest_invalid {
        for reason in &item.dangling_references {
            blockers.push(consequence(&key_of(item), reason, vec![]));
        }
    }
    for item in &admission.missing {
        blockers.push(consequence(&key_of(item), "missing_provenance", vec![]));
    }
    // The legacy missing-provenance helper predates these source families.
    // Check exact captured row backing without inventing new semantics.
    for key in overlay
        .sources
        .keys()
        .filter(|key| matches!(key.0.as_str(), "model_swap" | "standing_rule"))
    {
        if !rows.contains_key(key) {
            blockers.push(consequence(key, "missing_provenance", vec![]));
        }
    }
    consequences.extend(blockers.iter().cloned());
    exclude(&mut overlay, &consequences);
    let mut learned = captured.learned.clone();
    if epoch_changed {
        for row in learned.iter_mut().filter(|row| {
            row.kind != "persona"
                && matches!(
                    row.compatibility,
                    CompatibilityStatus::Compatible | CompatibilityStatus::OwnerAccepted
                )
        }) {
            let key = (row.kind.clone(), row.artifact_id.clone(), row.version);
            if overlay_compat::registry_entry_active_at(&overlay, &key.0, &key.1, key.2) {
                row.compatibility = CompatibilityStatus::ReconfirmationRequired;
                consequences.push(consequence(
                    &key,
                    "base_epoch_reconfirmation_required",
                    vec![],
                ));
            }
        }
    }
    let owner_sources = captured
        .registry
        .sources
        .iter()
        .map(|(key, source)| (key.clone(), source.bytes.clone()))
        .collect();
    let existing_invalid: Vec<_> = consequences.iter().map(orphan).collect();
    let mut merged = base.clone();
    artifact_loader::merge_registry(&mut merged, overlay);
    let (ordinary, accepted) = overlay_convergence::evaluate_captured_dependencies(
        &mut merged,
        &learned,
        &base_ids,
        &existing_invalid,
        &owner_sources,
    );
    for item in ordinary.into_iter().chain(accepted) {
        // Already recorded control facts must keep their original reasons.
        if existing_invalid.contains(&item) {
            continue;
        }
        let reason = if item.dangling_references == ["reconfirmation_required"] {
            "reconfirmation_required"
        } else {
            "dangling_dependencies"
        };
        consequences.push(consequence(
            &key_of(&item),
            reason,
            item.dangling_references,
        ));
    }
    for route in base
        .routes
        .iter()
        .filter(|r| r.lifecycle_state == Lifecycle::Active)
    {
        let refs =
            overlay_compat::dangling_for_parsed(&merged, &ParsedProposal::Route(route.clone()));
        if !refs.is_empty() {
            blockers.push(consequence(
                &("route".into(), route.id.clone(), route.version),
                "base_dangling_dependencies",
                refs,
            ));
        }
    }
    for workflow in base
        .workflows
        .values()
        .filter(|w| w.lifecycle_state == Lifecycle::Active)
    {
        let refs = overlay_compat::dangling_for_parsed(
            &merged,
            &ParsedProposal::Workflow(workflow.clone()),
        );
        if !refs.is_empty() {
            blockers.push(consequence(
                &("workflow".into(), workflow.id.clone(), workflow.version),
                "base_dangling_dependencies",
                refs,
            ));
        }
    }
    consequences.extend(blockers.iter().cloned());
    canonicalize(&mut consequences);
    canonicalize(&mut blockers);
    let effective_artifacts = captured
        .controls
        .keys()
        .filter(|key| {
            !base_ids.contains(&(key.0.clone(), key.1.clone()))
                && overlay_compat::registry_entry_active_at(&merged, &key.0, &key.1, key.2)
        })
        .cloned()
        .collect();
    Ok((
        OverlayViewAssessment {
            effective_artifacts,
            consequences,
        },
        blockers,
    ))
}

fn key_of(item: &OrphanedArtifact) -> ArtifactVersion {
    (item.kind.clone(), item.artifact_id.clone(), item.version)
}
fn consequence(key: &ArtifactVersion, reason: &str, details: Vec<String>) -> OverlayConsequence {
    OverlayConsequence {
        kind: key.0.clone(),
        artifact_id: key.1.clone(),
        version: key.2,
        reason: reason.into(),
        details,
    }
}
fn orphan(item: &OverlayConsequence) -> OrphanedArtifact {
    OrphanedArtifact {
        kind: item.kind.clone(),
        artifact_id: item.artifact_id.clone(),
        version: item.version,
        dangling_references: if item.details.is_empty() {
            vec![item.reason.clone()]
        } else {
            item.details.clone()
        },
    }
}
fn exclude(registry: &mut ArtifactRegistry, consequences: &[OverlayConsequence]) {
    overlay_compat::exclude_orphans(
        registry,
        &consequences.iter().map(orphan).collect::<Vec<_>>(),
    );
}
fn canonicalize(items: &mut Vec<OverlayConsequence>) {
    for item in items.iter_mut() {
        item.details.sort();
    }
    items.sort();
    items.dedup();
}

fn fingerprint(captured: &CapturedOverlayState) -> String {
    let mut learned: Vec<_> = captured
        .learned
        .iter()
        .map(|row| {
            (
                (row.kind.clone(), row.artifact_id.clone(), row.version),
                serde_json::json!({
                    "namespace": row.namespace, "provenance": row.provenance,
                    "accepted_via": row.accepted_via, "learned_at": row.learned_at,
                    "compatibility": format!("{:?}", row.compatibility),
                    "nomination": format!("{:?}", row.nomination),
                    "pending_reconfirmation_id": row.pending_reconfirmation_id,
                    "pending_yaml_digest": row.pending_yaml_digest,
                    "accepted_dependency_fingerprint": row.accepted_dependency_fingerprint,
                    "accepted_base_epoch": row.accepted_base_epoch,
                }),
            )
        })
        .collect();
    learned.sort_by(|a, b| a.0.cmp(&b.0));
    let controls: Vec<_> = captured.controls.iter().map(|(key, control)| serde_json::json!({
        "identity": key, "lifecycle": control.lifecycle,
        "highest_active_version": control.highest_active_version,
        "approved_yaml_digest": control.approved_yaml_digest,
        "source_present": control.source_present, "recoverable_blob_present": control.recoverable_blob_present,
    })).collect();
    let sources: BTreeMap<_, _> = captured
        .registry
        .sources
        .iter()
        .map(|(key, source)| (key, digest_of_bytes(&source.bytes)))
        .collect();
    let persona_expected: Vec<_> = captured.persona_findings.expected_digests.iter().collect();
    let persona_excluded: Vec<_> = captured
        .persona_findings
        .excluded
        .iter()
        .map(|(key, reason)| (key, format!("{reason:?}")))
        .collect();
    digest_of(&serde_json::json!({
        "schema_version": 1, "learned": learned, "controls": controls,
        "sources": sources.into_iter().collect::<Vec<_>>(),
        "source_inventory_digest": captured.source_inventory_digest,
        "persona_expected": persona_expected, "persona_excluded": persona_excluded,
    }))
    .to_string()
}

#[cfg(test)]
#[path = "review_overlay_tests.rs"]
mod tests;
