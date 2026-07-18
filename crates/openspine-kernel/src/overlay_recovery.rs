use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::artifact_loader::ArtifactRegistry;
use crate::artifact_store::ArtifactStore;
use crate::store::learned_artifacts::LearnedArtifact;
use crate::store::Store;

fn activate_overlay_yaml(
    registry: &mut ArtifactRegistry,
    kind: &str,
    yaml: &str,
) -> anyhow::Result<bool> {
    if kind == "template" {
        let mut template: crate::model_gateway::PromptTemplate = serde_yaml::from_str(yaml)?;
        template.lifecycle_state = openspine_schemas::artifact::Lifecycle::Active;
        let id = template.id.clone();
        let is_newer = registry
            .templates
            .get(&id)
            .map(|existing| existing.version < template.version)
            .unwrap_or(true);
        if is_newer {
            registry.templates.insert(id, template);
            return Ok(true);
        }
        return Ok(false);
    }
    let mut parsed = crate::artifact_loader::parse_proposal(kind, yaml)?;
    parsed.activate();
    parsed.insert_into(registry).map(|_| true)
}

pub(crate) fn republish_missing_committed(
    overlay_registry: &mut ArtifactRegistry,
    learned: &[LearnedArtifact],
    overlay_dir: &Path,
    store: &Store,
    artifacts: &ArtifactStore,
) -> anyhow::Result<Vec<(ulid::Ulid, String)>> {
    let mut recovered_reconfirmations = Vec::new();
    for row in learned.iter() {
        if !matches!(
            row.compatibility,
            crate::store::learned_artifacts::CompatibilityStatus::Compatible
                | crate::store::learned_artifacts::CompatibilityStatus::OwnerAccepted
        ) {
            continue;
        }
        let approved_dangling = store
            .find_proposed_artifact(&row.kind, &row.artifact_id, row.version)?
            .map(|proposal| proposal.state == openspine_schemas::artifact::Lifecycle::Approved)
            .unwrap_or(false);
        let active = store.is_active_proposal(&row.kind, &row.artifact_id, row.version)?
            && store.highest_active_version(&row.kind, &row.artifact_id)? == Some(row.version);
        if !active && !approved_dangling {
            continue;
        }
        // Only the DB-highest Active version may be recovered. A lower
        // approved-dangling version must not be revived when a higher version
        // is active — no silent rollback to a stale version (AD-070).
        if let Some(highest) = store.highest_active_version(&row.kind, &row.artifact_id)? {
            if highest > row.version {
                continue;
            }
        }
        let expected_path = row
            .source_path
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                overlay_dir
                    .join(crate::artifact_loader::overlay_subdir_for_kind(&row.kind).unwrap_or(""))
                    .join(crate::artifact_loader::overlay_filename(
                        &row.artifact_id,
                        row.version,
                    ))
            });
        if expected_path.exists() && !approved_dangling {
            continue;
        }
        let Some(digest) = row.pending_yaml_digest.as_deref() else {
            continue;
        };
        let Ok(parsed_digest) = openspine_schemas::digest::Digest::parse(digest.to_owned()) else {
            store.append_audit(
                "artifact.activation_republish_missing_ref",
                None,
                None,
                Some("committed overlay has no recoverable content ref"),
                None,
                &[],
                &[],
            )?;
            continue;
        };
        let review_ref = openspine_schemas::artifact::ArtifactRef {
            digest: parsed_digest,
            schema_version: 1,
        };
        let bytes = match artifacts.get(&review_ref) {
            Ok(bytes) => bytes,
            Err(_) => {
                store.append_audit(
                    "artifact.activation_republish_missing_bytes",
                    None,
                    None,
                    Some("committed overlay content is missing from the artifact store"),
                    None,
                    &[],
                    &[],
                )?;
                continue;
            }
        };
        let yaml = String::from_utf8_lossy(&bytes);
        let value: serde_yaml::Value = match serde_yaml::from_str(&yaml) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if value.get("id").and_then(serde_yaml::Value::as_str) != Some(row.artifact_id.as_str())
            || value.get("version").and_then(serde_yaml::Value::as_u64) != Some(row.version as u64)
        {
            store.append_audit(
                "artifact.activation_republish_identity_mismatch",
                None,
                None,
                Some("committed overlay content identity differs from learned row"),
                None,
                &[],
                &[],
            )?;
            continue;
        }
        let exact_loaded = approved_dangling
            && overlay_registry
                .sources
                .get(&(row.kind.clone(), row.artifact_id.clone(), row.version))
                .map(|source| source.bytes == bytes)
                .unwrap_or(false);
        let mut scratch = overlay_registry.clone();
        let inserted = match activate_overlay_yaml(&mut scratch, &row.kind, &yaml) {
            Ok(inserted) => inserted,
            Err(_) if exact_loaded => false,
            Err(_) => {
                store.append_audit(
                    "artifact.activation_republish_parse_failed",
                    None,
                    None,
                    Some("committed overlay failed to re-parse during recovery"),
                    None,
                    &[],
                    &[],
                )?;
                continue;
            }
        };
        if !inserted && !exact_loaded {
            continue;
        }
        std::fs::create_dir_all(expected_path.parent().unwrap())?;
        let tmp_path = expected_path.with_extension("tmp.republish");
        {
            let mut file = std::fs::File::create(&tmp_path)?;
            use std::io::Write as _;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp_path, &expected_path)?;
        if !approved_dangling {
            *overlay_registry = scratch;
            overlay_registry.sources.insert(
                (row.kind.clone(), row.artifact_id.clone(), row.version),
                crate::artifact_loader::ArtifactSource {
                    path: expected_path.clone(),
                    bytes: bytes.clone(),
                },
            );
        }
        if approved_dangling {
            let request_id = crate::overlay_compat::ensure_reconfirm_request(
                store,
                &row.kind,
                &row.artifact_id,
                row.version,
                row.pending_reconfirmation_id
                    .unwrap_or_else(ulid::Ulid::new),
                review_ref.clone(),
            )?;
            store.mark_reconfirmation_required(
                &row.kind,
                &row.artifact_id,
                row.version,
                request_id,
                review_ref.digest.as_str(),
            )?;
            remove_loaded_version(overlay_registry, &row.kind, &row.artifact_id, row.version);
            recovered_reconfirmations.push((
                request_id,
                format!("{}:{} v{}", row.kind, row.artifact_id, row.version),
            ));
        }
    }
    Ok(recovered_reconfirmations)
}

/// Drop any overlay version that is loaded on disk but is NOT the DB-highest
/// Active version for its identity, so a stale on-disk version (e.g. v1) can
/// never be merged live when the durable record says v2 is active (AD-070).
/// Versions whose proposal is not `active` at all are also dropped: an on-disk
/// artifact with only a `proposed`/`approved` row has no live authority.
/// Returns the excluded identity pairs so the caller can flag them.
pub(crate) fn prune_non_highest_active(
    overlay_registry: &mut ArtifactRegistry,
    store: &Store,
) -> anyhow::Result<HashSet<(String, String)>> {
    let learned = store.list_learned_artifacts()?;
    let eligible_personas: HashSet<(String, u32)> = overlay_registry
        .sources
        .iter()
        .filter_map(|((kind, id, version), source)| {
            if kind != "persona" {
                return None;
            }
            learned
                .iter()
                .any(|row| {
                    row.kind == "persona"
                        && row.artifact_id == *id
                        && row.version == *version
                        && matches!(
                            &row.provenance,
                            crate::store::learned_artifacts::Provenance::ProducedBy { .. }
                        )
                        && row.pending_yaml_digest.as_deref()
                            == Some(
                                openspine_schemas::digest::digest_of_bytes(&source.bytes).as_str(),
                            )
                })
                .then_some((id.clone(), *version))
        })
        .collect();
    let mut excluded: HashSet<(String, String)> =
        crate::artifact_loader::exclude_unbacked_persona_versions(
            overlay_registry,
            &eligible_personas,
        )?
        .into_iter()
        .map(|(id, _version)| ("persona".to_string(), id))
        .collect();
    let identities: HashSet<(String, String)> = overlay_registry
        .sources
        .keys()
        .map(|(k, id, _v)| (k.clone(), id.clone()))
        .collect();
    for (kind, artifact_id) in identities {
        if kind == "persona" {
            // Valid persona versions are retained by the row-and-digest
            // admission pass above; personas have no proposal lifecycle.
            continue;
        }
        let highest = store.highest_active_version(&kind, &artifact_id)?;
        let loaded: Vec<u32> = overlay_registry
            .sources
            .iter()
            .filter(|((k, id, _v), _)| k == &kind && id == &artifact_id)
            .map(|((_k, _id, v), _)| *v)
            .collect();
        match highest {
            // No active proposal: every loaded version is stale, drop all.
            None => {
                for v in loaded {
                    remove_loaded_version(overlay_registry, &kind, &artifact_id, v);
                }
                excluded.insert((kind, artifact_id));
            }
            Some(highest) => {
                for v in loaded {
                    if v != highest {
                        remove_loaded_version(overlay_registry, &kind, &artifact_id, v);
                        excluded.insert((kind.clone(), artifact_id.clone()));
                    }
                }
                if crate::artifact_loader::artifact_version(overlay_registry, &kind, &artifact_id)
                    != Some(highest)
                {
                    if let Some(source) = overlay_registry
                        .sources
                        .get(&(kind.clone(), artifact_id.clone(), highest))
                        .cloned()
                    {
                        crate::artifact_loader::rehydrate_source(overlay_registry, &kind, &source)?;
                    }
                }
            }
        }
    }
    Ok(excluded)
}

/// Remove a specific `(kind, id, version)` from both the versioned `sources`
/// map and the appropriate typed registry collection, so pruning is exact
/// (it never drops a higher version that shares the identity).
fn remove_loaded_version(registry: &mut ArtifactRegistry, kind: &str, id: &str, version: u32) {
    registry
        .sources
        .remove(&(kind.to_string(), id.to_string(), version));
    match kind {
        "route" => registry
            .routes
            .retain(|r| !(r.id == id && r.version == version)),
        "agent" => registry
            .agents
            .retain(|k, a| !(k == id && a.version == version)),
        "workflow" => registry
            .workflows
            .retain(|k, w| !(k == id && w.version == version)),
        "pack" => registry
            .packs
            .retain(|k, p| !(k == id && p.version == version)),
        "policy" => registry
            .policies
            .retain(|k, p| !(k == id && p.version == version)),
        "template" => registry
            .templates
            .retain(|k, t| !(k == id && t.version == version)),
        "model_swap" => registry
            .model_swaps
            .retain(|k, m| !(k == id && m.version == version)),
        _ => {}
    }
}

#[cfg(test)]
#[path = "overlay_recovery_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "overlay_recovery_regression_tests.rs"]
mod regressions;
