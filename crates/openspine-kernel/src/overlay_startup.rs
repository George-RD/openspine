use anyhow::Context as _;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::artifact_loader::{self, ArtifactRegistry};
use crate::artifact_store::ArtifactStore;
use crate::store::Store;

pub(crate) struct OverlayStartup {
    pub registry: ArtifactRegistry,
    pub base_artifact_ids: HashSet<(String, String)>,
    pub base_compatibility_epoch: String,
    pub overlay_dir: PathBuf,
    pub pending_reconfirm_buttons: Vec<(ulid::Ulid, String)>,
    pub pending_reconfirm_notices: Vec<String>,
}

/// Remove staged-but-uncommitted overlay temp files left by a crash between
/// fsync and the durable commit (AD-070 crash ordering). The activation
/// pipeline stages `final_path.with_extension("tmp.<ULID>")` and only renames
/// it to the published `.yaml` after `commit_artifact_activation` returns, so
/// a crash leaves an unrenamed `*.tmp.<ULID>` file. Startup must discard it
/// rather than let it accumulate; the previously published `.yaml` (if any)
/// is the effective state and is never touched. Scoped to the six overlay
/// subdirectories so unrelated temp files are never removed.
fn discard_staged_overlay_files(overlay_dir: &Path) -> anyhow::Result<()> {
    for subdir in [
        "routes",
        "agents",
        "workflows",
        "packs",
        "policies",
        "templates",
    ] {
        let dir = overlay_dir.join(subdir);
        if !dir.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            // Only the activation pipeline's pre-commit stage file carries the
            // exact `*.tmp.<ULID>` suffix (`row.id`); recovery's `.tmp.republish`
            // and the artifact store's `.tmp.<hex>` are distinct and untouched.
            if let Some((_, candidate)) = name.rsplit_once(".tmp.") {
                if ulid::Ulid::from_string(candidate).is_ok() {
                    std::fs::remove_file(path)?;
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn load(
    lyra_dir: &Path,
    data_dir: &Path,
    store: &Store,
    artifacts: &ArtifactStore,
) -> anyhow::Result<OverlayStartup> {
    let mut registry = artifact_loader::load_registry(lyra_dir)
        .with_context(|| format!("loading artifact registry from {}", lyra_dir.display()))?;
    let base_artifact_ids = artifact_loader::artifact_identity_pairs(&registry);
    let base_compatibility_epoch =
        crate::overlay_compat::compatibility_epoch(&registry, &base_artifact_ids);
    let overlay_dir = data_dir.join("artifacts.d");
    discard_staged_overlay_files(&overlay_dir).with_context(|| {
        format!(
            "discarding staged overlay files in {}",
            overlay_dir.display()
        )
    })?;
    // On a real first boot, ship the AD-153 seed workflow set into the overlay
    // namespace. Gated to non-test builds so the shared `load` used by unit
    // tests (including overlay-recovery tests) does not materialize files into
    // their fixtures. `materialize_missing` records a persisted marker, so it
    // runs exactly once per fresh install: a seed the owner deletes is not
    // re-created, and an owner-upgraded higher version stays live
    // (highest-version-wins). An existing file is never overwritten.
    #[cfg(not(test))]
    crate::seed_workflows::materialize_missing(store, &overlay_dir).with_context(|| {
        format!(
            "materializing seed workflows into {}",
            overlay_dir.display()
        )
    })?;
    let mut overlay_registry = artifact_loader::load_registry(&overlay_dir)
        .with_context(|| format!("loading artifact overlay from {}", overlay_dir.display()))?;
    for (kind, subdir) in [
        ("route", "routes"),
        ("agent", "agents"),
        ("workflow", "workflows"),
        ("pack", "packs"),
        ("policy", "policies"),
        ("template", "templates"),
    ] {
        let dir = overlay_dir.join(subdir);
        if !dir.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            if !path
                .extension()
                .is_some_and(|ext| ext == "yaml" || ext == "yml")
            {
                continue;
            }
            let value: serde_yaml::Value = serde_yaml::from_str(&std::fs::read_to_string(&path)?)?;
            let Some(id) = value.get("id").and_then(serde_yaml::Value::as_str) else {
                continue;
            };
            let Some(version) = value
                .get("version")
                .and_then(serde_yaml::Value::as_u64)
                .map(|value| value as u32)
            else {
                continue;
            };
            if artifact_loader::artifact_version(&overlay_registry, kind, id)
                .is_some_and(|current| version < current)
            {
                let marker = format!("artifact.superseded:{kind}:{id}:v{version}");
                if store.get_kv(&marker)?.is_none() {
                    store.append_audit(
                        "artifact.superseded",
                        None,
                        None,
                        Some(&format!(
                            "{kind}:{id} v{version} superseded by higher activated version"
                        )),
                        None,
                        &[],
                        &[],
                    )?;
                    store.set_kv(&marker, "recorded")?;
                }
            }
        }
    }
    let overlay_artifact_ids = artifact_loader::artifact_identity_pairs(&overlay_registry);
    let collisions = overlay_artifact_ids
        .intersection(&base_artifact_ids)
        .cloned()
        .collect::<HashSet<_>>();
    let mut learned = store
        .list_learned_artifacts()
        .context("loading learned artifact provenance")?;
    let collision_orphans: Vec<crate::overlay_compat::OrphanedArtifact> = collisions
        .iter()
        .filter_map(|(kind, artifact_id)| {
            let version = artifact_loader::artifact_version(&overlay_registry, kind, artifact_id)?;
            learned
                .iter()
                .find(|item| {
                    item.kind == *kind
                        && item.artifact_id == *artifact_id
                        && item.version == version
                })
                .map(|_| crate::overlay_compat::OrphanedArtifact {
                    kind: kind.clone(),
                    artifact_id: artifact_id.clone(),
                    version,
                    dangling_references: vec!["base_overlay_collision".into()],
                })
        })
        .collect();
    let digest_invalid: Vec<crate::overlay_compat::OrphanedArtifact> = learned
        .iter()
        .filter(|item| {
            matches!(
                item.compatibility,
                crate::store::learned_artifacts::CompatibilityStatus::Compatible
                    | crate::store::learned_artifacts::CompatibilityStatus::OwnerAccepted
            ) && artifact_loader::artifact_version(&overlay_registry, &item.kind, &item.artifact_id)
                == Some(item.version)
        })
        .filter_map(|item| {
            let source = overlay_registry.sources.get(&(
                item.kind.clone(),
                item.artifact_id.clone(),
                item.version,
            ))?;
            let Some(expected) = item.pending_yaml_digest.as_deref() else {
                return Some(crate::overlay_compat::OrphanedArtifact {
                    kind: item.kind.clone(),
                    artifact_id: item.artifact_id.clone(),
                    version: item.version,
                    dangling_references: vec!["approved_overlay_digest_missing".into()],
                });
            };
            let actual = openspine_schemas::digest::digest_of_bytes(&source.bytes);
            (expected != actual.as_str()).then(|| crate::overlay_compat::OrphanedArtifact {
                kind: item.kind.clone(),
                artifact_id: item.artifact_id.clone(),
                version: item.version,
                dangling_references: vec!["approved_overlay_digest_mismatch".into()],
            })
        })
        .collect();
    artifact_loader::exclude_identity_pairs(
        &mut overlay_registry,
        &digest_invalid
            .iter()
            .map(|item| (item.kind.clone(), item.artifact_id.clone()))
            .collect(),
    );
    let missing = crate::overlay_compat::missing_provenance(&overlay_registry, &learned);
    for artifact in &missing {
        let source = overlay_registry
            .sources
            .get(&(
                artifact.kind.clone(),
                artifact.artifact_id.clone(),
                artifact.version,
            ))
            .ok_or_else(|| anyhow::anyhow!("legacy overlay source is missing"))?;
        let yaml_path = source.path.clone();
        let bytes = source.bytes.clone();
        let row = crate::store::learned_artifacts::LearnedArtifact {
            kind: artifact.kind.clone(),
            artifact_id: artifact.artifact_id.clone(),
            version: artifact.version,
            namespace: openspine_schemas::artifact::ArtifactNamespace::Overlay,
            provenance: crate::store::learned_artifacts::Provenance::LegacyMigration {
                discovered_at: jiff::Timestamp::now(),
            },
            accepted_via: None,
            learned_at: jiff::Timestamp::now(),
            compatibility:
                crate::store::learned_artifacts::CompatibilityStatus::ReconfirmationRequired,
            nomination: crate::store::learned_artifacts::NominationStatus::None,
            pending_reconfirmation_id: None,
            pending_yaml_digest: Some(
                openspine_schemas::digest::digest_of_bytes(&bytes).to_string(),
            ),
            accepted_dependency_fingerprint: None,
            source_path: Some(yaml_path.to_string_lossy().into_owned()),
            accepted_base_epoch: None,
        };
        store.record_learned_artifact(&row)?;
        learned.push(row);
    }
    // Drop stale on-disk versions that are not the DB-highest Active before
    // recovering any missing committed files (AD-070: no silent rollback to a
    // lower version when the highest-active bytes are missing).
    let _pruned = crate::overlay_recovery::prune_non_highest_active(&mut overlay_registry, store)?;

    let recovered_reconfirmations = crate::overlay_recovery::republish_missing_committed(
        &mut overlay_registry,
        &learned,
        &overlay_dir,
        store,
        artifacts,
    )?;
    for (kind, id) in &collisions {
        store.append_audit(
            "artifact.overlay_collision_reconfirmation_required",
            None,
            None,
            Some("base update introduced a namespace identity collision"),
            None,
            &[],
            &[],
        )?;
        tracing::warn!(kind = %kind, artifact_id = %id,
            "overlay collision excluded pending owner review");
    }
    artifact_loader::exclude_identity_pairs(&mut overlay_registry, &collisions);
    crate::overlay_compat::exclude_orphans(&mut overlay_registry, &missing);
    artifact_loader::merge_registry(&mut registry, std::mem::take(&mut overlay_registry));
    for artifact in &missing {
        store.append_audit(
            "artifact.excluded_missing_provenance",
            None,
            None,
            Some("overlay file has no durable learned-artifact provenance"),
            None,
            &[],
            &[],
        )?;
        tracing::warn!(kind = %artifact.kind, artifact_id = %artifact.artifact_id,
            "excluded legacy overlay artifact without provenance");
    }
    let mut owner_accepted_invalid = Vec::new();
    let prior_invalid_len = owner_accepted_invalid.len();
    let (orphans, review_ids, planned_invalid) =
        crate::overlay_compat::converge_owner_accepted_dependencies(
            &mut registry,
            &learned,
            &base_artifact_ids,
            &owner_accepted_invalid,
        );
    owner_accepted_invalid = planned_invalid;
    for _ in owner_accepted_invalid.iter().skip(prior_invalid_len) {
        store.append_audit(
            "artifact.owner_accepted_rejected",
            None,
            None,
            Some("owner-accepted dependency was excluded; typed references now dangling"),
            None,
            &[],
            &[],
        )?;
    }
    for item in learned.iter().filter(|item| {
        item.compatibility == crate::store::learned_artifacts::CompatibilityStatus::OwnerAccepted
            && !owner_accepted_invalid.iter().any(|orphan| {
                orphan.kind == item.kind
                    && orphan.artifact_id == item.artifact_id
                    && orphan.version == item.version
            })
            && item.accepted_base_epoch.as_deref() != Some(base_compatibility_epoch.as_str())
    }) {
        store.refresh_owner_accepted_epoch(
            &item.kind,
            &item.artifact_id,
            item.version,
            &base_compatibility_epoch,
        )?;
    }
    let (mut orphans, mut review_ids): (
        Vec<crate::overlay_compat::OrphanedArtifact>,
        Vec<ulid::Ulid>,
    ) = orphans
        .into_iter()
        .zip(review_ids)
        .filter(|(orphan, _)| {
            !base_artifact_ids.contains(&(orphan.kind.clone(), orphan.artifact_id.clone()))
        })
        .unzip();
    review_ids.extend(missing.iter().map(|_| ulid::Ulid::new()));
    orphans.extend(missing.iter().cloned());
    review_ids.extend(owner_accepted_invalid.iter().map(|_| ulid::Ulid::new()));
    orphans.extend(owner_accepted_invalid.clone());
    review_ids.extend(collisions.iter().filter_map(|(kind, artifact_id)| {
        learned
            .iter()
            .find(|item| item.kind == *kind && item.artifact_id == *artifact_id)
            .and_then(|item| item.pending_reconfirmation_id)
            .or_else(|| Some(ulid::Ulid::new()))
    }));
    orphans.extend(collision_orphans);
    review_ids.extend(digest_invalid.iter().map(|_| ulid::Ulid::new()));
    orphans.extend(digest_invalid.clone());
    let mut pending_reconfirm_buttons: Vec<(ulid::Ulid, String)> = Vec::new();
    let mut pending_reconfirm_notices: Vec<String> = Vec::new();
    pending_reconfirm_buttons.extend(recovered_reconfirmations);
    for (orphan, request_id) in orphans.iter().zip(review_ids) {
        let yaml_path = learned
            .iter()
            .find(|item| {
                item.kind == orphan.kind
                    && item.artifact_id == orphan.artifact_id
                    && item.version == orphan.version
            })
            .and_then(|item| item.source_path.as_deref())
            .map(PathBuf::from)
            .or_else(|| {
                registry
                    .sources
                    .get(&(
                        orphan.kind.clone(),
                        orphan.artifact_id.clone(),
                        orphan.version,
                    ))
                    .map(|source| source.path.clone())
            })
            .ok_or_else(|| anyhow::anyhow!("overlay review source is missing"))?;
        let yaml = std::fs::read(&yaml_path)
            .with_context(|| format!("reading overlay review payload {}", yaml_path.display()))?;
        if let Some(learned_row) = learned.iter().find(|item| {
            item.kind == orphan.kind
                && item.artifact_id == orphan.artifact_id
                && item.version == orphan.version
        }) {
            if learned_row
                .pending_yaml_digest
                .as_deref()
                .is_some_and(|digest| {
                    digest != openspine_schemas::digest::digest_of_bytes(&yaml).as_str()
                })
            {
                store.append_audit(
                    "artifact.reconfirm_digest_mismatch",
                    None,
                    None,
                    Some("overlay bytes differ from activation baseline"),
                    None,
                    &[],
                    &[],
                )?;
                pending_reconfirm_notices.push(format!(
                    "Overlay {} v{} changed after review; please re-propose it.",
                    orphan.artifact_id, orphan.version
                ));
                continue;
            }
        }
        let review_ref = artifacts.put(&yaml).context("storing review payload")?;
        let chosen_request_id = crate::overlay_compat::ensure_reconfirm_request(
            store,
            &orphan.kind,
            &orphan.artifact_id,
            orphan.version,
            request_id,
            review_ref.clone(),
        )
        .context("persisting reconfirm action request")?;
        store.mark_reconfirmation_required(
            &orphan.kind,
            &orphan.artifact_id,
            orphan.version,
            chosen_request_id,
            review_ref.digest.as_str(),
        )?;
        pending_reconfirm_buttons.push((
            chosen_request_id,
            format!(
                "Re-confirm overlay\nKind: {}\nId: {} v{}\nDigest: {}\n\nApprove to restore.",
                orphan.kind, orphan.artifact_id, orphan.version, review_ref.digest,
            ),
        ));
        tracing::warn!(
            kind = %orphan.kind,
            artifact_id = %orphan.artifact_id,
            version = orphan.version,
            request_id = %chosen_request_id,
            "learned artifact requires owner re-confirmation after update"
        );
        store.append_audit(
            "artifact.reconfirmation_required",
            None,
            None,
            Some("base update left a learned artifact with dangling references"),
            None,
            &[],
            &[],
        )?;
    }
    Ok(OverlayStartup {
        registry,
        base_artifact_ids,
        base_compatibility_epoch,
        overlay_dir,
        pending_reconfirm_buttons,
        pending_reconfirm_notices,
    })
}
