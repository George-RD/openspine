use crate::artifact_loader;
use crate::artifact_store::ArtifactStore;
use crate::store::Store;
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::{ArtifactRef, Lifecycle};
use openspine_schemas::digest::Digest;

fn quarantine_pending_overlay(
    store: &Store,
    path: &std::path::Path,
    reason: &'static str,
) -> anyhow::Result<()> {
    store.append_audit(
        "artifact.activation_recovery_failed",
        Some(&ActionId::new("artifact.activate")),
        None,
        Some(reason),
        None,
        &[],
        &[],
    )?;
    std::fs::rename(path, path.with_extension("quarantine"))?;
    Ok(())
}

pub(crate) fn reconcile_model_swap_overlay(
    store: &Store,
    artifacts: &ArtifactStore,
    overlay_dir: &std::path::Path,
) -> anyhow::Result<()> {
    let active = store
        .active_model_swap_ids()?
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    let dir = overlay_dir.join("model_swaps");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(());
    };
    for entry in entries {
        let path = entry?.path();
        let extension = path.extension().and_then(|ext| ext.to_str());
        if extension != Some("yaml") && extension != Some("pending") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|name| name.to_str()) else {
            if extension == Some("pending") {
                quarantine_pending_overlay(store, &path, "pending overlay filename is malformed")?;
            }
            continue;
        };
        let Some((id, version)) = stem.rsplit_once("-v") else {
            if extension == Some("pending") {
                quarantine_pending_overlay(store, &path, "pending overlay filename is malformed")?;
            }
            continue;
        };
        let Ok(version) = version.parse::<u32>() else {
            if extension == Some("pending") {
                quarantine_pending_overlay(store, &path, "pending overlay filename is malformed")?;
            }
            continue;
        };
        let is_active = active.contains(&(id.to_string(), version));
        if extension == Some("pending") {
            if !is_active {
                store.append_audit(
                    "artifact.activation_recovery_started",
                    Some(&ActionId::new("artifact.activate")),
                    None,
                    Some("discarding pre-commit pending overlay"),
                    None,
                    &[],
                    &[],
                )?;
                std::fs::remove_file(&path)?;
                store.append_audit(
                    "artifact.activation_recovered",
                    Some(&ActionId::new("artifact.activate")),
                    None,
                    Some("discarded pre-commit pending overlay"),
                    None,
                    &[],
                    &[],
                )?;
                continue;
            }

            let quarantine = || -> anyhow::Result<()> {
                quarantine_pending_overlay(
                    store,
                    &path,
                    "pending overlay failed provenance reconciliation",
                )
            };
            let pending_bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(_) => {
                    quarantine()?;
                    continue;
                }
            };
            let pending_yaml = match std::str::from_utf8(&pending_bytes) {
                Ok(yaml) => yaml,
                Err(_) => {
                    quarantine()?;
                    continue;
                }
            };
            let mut pending = match artifact_loader::parse_proposal("model_swap", pending_yaml) {
                Ok(parsed) => parsed,
                Err(_) => {
                    quarantine()?;
                    continue;
                }
            };
            if pending.artifact_id() != id
                || pending.version() != version
                || pending.lifecycle_state() != Lifecycle::Active
            {
                quarantine()?;
                continue;
            }
            let Some((state, reviewed_digest)) = store.find_proposed_artifact_state(
                "model_swap",
                pending.artifact_id(),
                pending.version(),
            )?
            else {
                quarantine()?;
                continue;
            };
            if state != Lifecycle::Active {
                quarantine()?;
                continue;
            }
            let reviewed_ref = ArtifactRef {
                digest: match Digest::parse(&reviewed_digest) {
                    Ok(digest) => digest,
                    Err(_) => {
                        quarantine()?;
                        continue;
                    }
                },
                schema_version: 1,
            };
            let reviewed_bytes = match artifacts.get(&reviewed_ref) {
                Ok(bytes) => bytes,
                Err(_) => {
                    quarantine()?;
                    continue;
                }
            };
            let reviewed_yaml = match std::str::from_utf8(&reviewed_bytes) {
                Ok(yaml) => yaml,
                Err(_) => {
                    quarantine()?;
                    continue;
                }
            };
            let mut reviewed = match artifact_loader::parse_proposal("model_swap", reviewed_yaml) {
                Ok(parsed) => parsed,
                Err(_) => {
                    quarantine()?;
                    continue;
                }
            };
            if reviewed.kind() != "model_swap"
                || reviewed.artifact_id() != id
                || reviewed.version() != version
            {
                quarantine()?;
                continue;
            }
            reviewed.activate();
            pending.activate();
            if reviewed.to_yaml()? != pending.to_yaml()? {
                quarantine()?;
                continue;
            }
            store.append_audit(
                "artifact.activation_recovery_started",
                Some(&ActionId::new("artifact.activate")),
                None,
                Some("completing post-commit pending overlay"),
                None,
                &[],
                std::slice::from_ref(&reviewed_ref),
            )?;
            std::fs::rename(&path, path.with_extension("yaml"))?;
            store.append_audit(
                "artifact.activation_recovered",
                Some(&ActionId::new("artifact.activate")),
                None,
                Some("completed post-commit pending overlay"),
                None,
                &[],
                std::slice::from_ref(&reviewed_ref),
            )?;
        } else if !is_active {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}
