static ACTIVATION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
use crate::store::learned_artifacts::{CompatibilityStatus, LearnedArtifact, NominationStatus};
use jiff::Timestamp;
use openspine_schemas::action::{ActionId, ActionRequest};
use openspine_schemas::artifact::ArtifactNamespace;
use openspine_schemas::grant::TaskGrant;

use super::notify_owner_best_effort;
use super::AppState;

/// Overlay so it survives a restart, and insert it into the live registry
/// so it participates in routing/composition immediately.
pub(super) async fn activate_approved_artifact(
    state: &AppState,
    grant: &TaskGrant,
    request: &ActionRequest,
    chat_id: i64,
) -> anyhow::Result<()> {
    let _activation_guard = ACTIVATION_LOCK.lock().await;
    let Some(row) = state
        .store
        .find_proposed_artifact_by_action_request(request.id)?
    else {
        state.store.append_audit(
            "artifact.activation_failed",
            Some(&request.action),
            None,
            Some("no proposed_artifacts row for this action request"),
            Some(grant.id),
            &[],
            &[],
        )?;
        drop(_activation_guard);
        notify_owner_best_effort(state, chat_id, "That artifact proposal is no longer valid.")
            .await;
        return Ok(());
    };

    let payload_ref = request
        .payload_ref
        .as_ref()
        .expect("checked by dispatch_artifact_propose before dispatch");
    let bytes = state.artifacts.get(payload_ref)?;
    let yaml = std::str::from_utf8(&bytes)?;

    let mut parsed = match crate::artifact_loader::parse_proposal(&row.kind, yaml) {
        Ok(parsed) => parsed,
        Err(err) => {
            state.store.append_audit(
                "artifact.activation_failed",
                Some(&request.action),
                None,
                Some("reparse_failed"),
                Some(grant.id),
                &[],
                &[],
            )?;
            tracing::warn!(error = %err, "approved artifact proposal failed to re-parse");
            drop(_activation_guard);
            notify_owner_best_effort(
                state,
                chat_id,
                "Approved, but the artifact could not be re-validated — activation aborted.",
            )
            .await;
            return Ok(());
        }
    };
    let model_swap_target = if let crate::artifact_loader::ParsedProposal::ModelSwap(swap) = &parsed
    {
        let golden_set = state
            .registry
            .read()
            .golden_sets
            .get(&swap.golden_set_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("trusted golden set disappeared before activation"))?;
        if !golden_set.roles.contains(&swap.role) {
            anyhow::bail!("trusted golden set is not authorized for this model role");
        }
        if !state.provider_pool.contains_key(&swap.target_provider_id) {
            anyhow::bail!("candidate provider disappeared before activation");
        }
        let provider_digest = state
            .provider_config_digests
            .get(&swap.target_provider_id)
            .ok_or_else(|| anyhow::anyhow!("candidate provider configuration disappeared"))?;
        crate::model_swap::verify_activation_binding(swap, &golden_set, provider_digest)?;
        let provider_id = swap.target_provider_id.clone();
        let role = swap.role;
        let current_version = state
            .registry
            .read()
            .model_swaps
            .get(&swap.id)
            .map(|current| current.version);
        if current_version.is_some_and(|current| current >= swap.version) {
            anyhow::bail!("model swap version is no longer newer than the active version");
        }
        Some((role, provider_id))
    } else {
        None
    };

    parsed.activate();
    let yaml_text = parsed
        .to_yaml()
        .expect("a value this crate just deserialized always re-serializes");

    let artifact_id = parsed.artifact_id().to_string();
    let version = parsed.version();
    // Dangling references gate the proposal's final lifecycle state: a
    // dangling overlay still activates (and is published) but stays in
    // `Approved` with a pending reconfirmation until the owner taps; a clean
    // overlay advances straight to `Active`. Either way the durable commit
    // below records the truth before any file is published.
    let dangling = {
        let registry = state.registry.read();
        crate::overlay_compat::dangling_for_parsed(&registry, &parsed)
    };
    let superseded_old_version = {
        let registry = state.registry.read();
        crate::artifact_loader::artifact_version(&registry, &row.kind, &artifact_id)
            .filter(|current| *current < version)
    };
    if crate::artifact_loader::artifact_version(&state.registry.read(), &row.kind, &artifact_id)
        .is_some_and(|current| current >= version)
    {
        state.store.append_audit(
            "artifact.activation_failed",
            Some(&request.action),
            None,
            Some("artifact version is not newer than active version"),
            Some(grant.id),
            &[],
            &[],
        )?;
        notify_owner_best_effort(
            state,
            chat_id,
            "That version is not newer than what is already active.",
        )
        .await;
        return Ok(());
    }
    // Persist the exact active bytes in the encrypted artifact store before
    // the SQLite commit; startup recovery can therefore republish after a
    // commit-before-rename crash using this digest-bound ref.
    let active_ref = state.artifacts.put(yaml_text.as_bytes())?;
    // Stage the overlay file to a temp name and fsync it before the durable
    // commit (AD-070 crash ordering): a crash between these two steps leaves
    // only an unrenamed temp file and an uncommitted proposal, which startup
    // ignores. The rename/publish happens only after the commit returns.
    let subdir = state.overlay_dir.join(parsed.overlay_subdir());
    std::fs::create_dir_all(&subdir)?;
    let final_path = subdir.join(crate::artifact_loader::overlay_filename(
        parsed.artifact_id(),
        parsed.version(),
    ));
    let tmp_path = if model_swap_target.is_some() {
        final_path.with_extension("pending")
    } else {
        final_path.with_extension(format!("tmp.{}", row.id))
    };
    {
        let mut file = std::fs::File::create(&tmp_path)?;
        use std::io::Write as _;
        file.write_all(yaml_text.as_bytes())?;
        file.sync_all()?;
    }
    let (_, source_exchange, _) = state
        .store
        .find_task_grant_by_id(row.task_grant_id)?
        .ok_or_else(|| anyhow::anyhow!("grant for learned artifact provenance is missing"))?;
    state.store.set_proposed_artifact_state(
        row.id,
        openspine_schemas::artifact::Lifecycle::ReviewRequired,
        openspine_schemas::artifact::Lifecycle::Approved,
    )?;
    let committed =
        state
            .store
            .commit_artifact_activation(crate::store::activation::ActivationCommit {
                learned: LearnedArtifact {
                    kind: parsed.kind().to_string(),
                    artifact_id: artifact_id.clone(),
                    version,
                    namespace: ArtifactNamespace::Overlay,
                    provenance: crate::store::learned_artifacts::Provenance::ProducedBy {
                        source_event_id: grant.event_id,
                        source_exchange,
                    },
                    accepted_via: None,
                    learned_at: Timestamp::now(),
                    compatibility: CompatibilityStatus::Compatible,
                    pending_yaml_digest: Some(active_ref.digest.to_string()),
                    accepted_dependency_fingerprint: None,
                    nomination: NominationStatus::None,
                    pending_reconfirmation_id: None,
                    source_path: None,
                    accepted_base_epoch: None,
                },
                proposed_id: row.id,
                grant_id: Some(grant.id),
                payload_ref: Some(payload_ref.clone()),
                dangling: !dangling.is_empty(),
                superseded_old_version,
            })?;
    if !committed {
        let _ = std::fs::remove_file(&tmp_path);
        return Ok(());
    }
    // Durable commit succeeded (learned row + proposal Active transition +
    // activation/superseded audits inside one transaction). Publish now:
    // atomic rename, then the only in-memory mutation. No fallible I/O or
    // DB write follows, so a crash here is recovered by startup republish.
    std::fs::rename(&tmp_path, &final_path)?;
    if !dangling.is_empty() {
        let review_ref = state.artifacts.put(yaml_text.as_bytes())?;
        let request_id = ulid::Ulid::new();
        state.store.mark_reconfirmation_required(
            parsed.kind(),
            parsed.artifact_id(),
            parsed.version(),
            request_id,
            review_ref.digest.as_str(),
        )?;
        let target_digest = openspine_schemas::digest::digest_of(&serde_json::json!({
            "kind": parsed.kind(),
            "artifact_id": parsed.artifact_id(),
            "version": parsed.version(),
        }));
        let reconfirm_request = ActionRequest {
            id: request_id,
            task_grant_id: ulid::Ulid::new(),
            action: ActionId::new("artifact.reconfirm"),
            target_ref: None,
            payload_ref: Some(review_ref.clone()),
            target_digest: Some(target_digest),
            selection_token_id: None,
            requested_at: Timestamp::now(),
            schema_version: 1,
        };
        state.store.insert_action_request(&reconfirm_request)?;
        state
            .connectors
            .telegram()
            .send_reply_with_approval_button(
                chat_id,
                &format!(
                    "Re-confirm overlay\nKind: {}\nId: {} v{}\nDigest: {}\n\nApprove to restore.",
                    parsed.kind(),
                    parsed.artifact_id(),
                    parsed.version(),
                    review_ref.digest,
                ),
                request_id,
            )
            .await?;
        state.store.append_audit(
            "artifact.reconfirmation_required",
            Some(&request.action),
            None,
            Some("activation found dangling references"),
            Some(grant.id),
            &[],
            std::slice::from_ref(payload_ref),
        )?;
        notify_owner_best_effort(
            state,
            chat_id,
            "Artifact needs owner re-confirmation because references are unavailable.",
        )
        .await;
        return Ok(());
    }
    {
        let mut registry = state.registry.write();
        if let Err(err) = parsed.insert_into(&mut registry) {
            anyhow::bail!("activation publication failed after durable commit: {err}");
        }
    }
    if let Some((role, provider_id)) = model_swap_target {
        state
            .active_model_providers
            .write()
            .insert(role, provider_id);
    }
    drop(_activation_guard);
    notify_owner_best_effort(
        state,
        chat_id,
        &format!("Artifact {artifact_id} v{version} is now active."),
    )
    .await;
    Ok(())
}
