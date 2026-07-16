static ACTIVATION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
use openspine_schemas::action::ActionRequest;
use openspine_schemas::artifact::Lifecycle;
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
    let _activation_guard = ACTIVATION_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("activation lock poisoned"))?;
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

    state.store.set_proposed_artifact_state(
        row.id,
        Lifecycle::ReviewRequired,
        Lifecycle::Approved,
    )?;

    parsed.activate();
    let yaml_text = parsed
        .to_yaml()
        .expect("a value this crate just deserialized always re-serializes");

    // Model swaps use the crash-recoverable pending suffix. Generic artifact
    // kinds retain the prior final-YAML activation path.
    let is_model_swap = model_swap_target.is_some();
    let subdir = state.overlay_dir.join(parsed.overlay_subdir());
    std::fs::create_dir_all(&subdir)?;
    let final_path = subdir.join(format!(
        "{}-v{}.yaml",
        parsed.artifact_id(),
        parsed.version()
    ));
    let pending_path = final_path.with_extension("pending");
    if is_model_swap {
        std::fs::write(&pending_path, yaml_text.as_bytes())?;
    } else {
        let staged_path = final_path.with_extension("staged");
        std::fs::write(&staged_path, yaml_text.as_bytes())?;
        std::fs::rename(&staged_path, &final_path)?;
    }

    let activation_result =
        state
            .store
            .activate_with_audit(row.id, &request.action, grant.id, payload_ref);
    if let Err(err) = activation_result {
        let _ = std::fs::remove_file(if is_model_swap {
            &pending_path
        } else {
            &final_path
        });
        return Err(err.into());
    }
    if is_model_swap {
        std::fs::rename(&pending_path, &final_path)?;
    }
    let artifact_id = parsed.artifact_id().to_string();
    let version = parsed.version();
    {
        let mut registry = state.registry.write();
        parsed.insert_into(&mut registry);
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
