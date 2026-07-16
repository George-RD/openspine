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
        notify_owner_best_effort(state, chat_id, "That artifact proposal is no longer valid.")
            .await;
        return Ok(());
    };

    let payload_ref = request
        .payload_ref
        .as_ref()
        .expect("checked by dispatch_artifact_propose before dispatch");
    let bytes = state.artifacts.get(payload_ref)?;
    let yaml = String::from_utf8_lossy(&bytes);

    let mut parsed = match crate::artifact_loader::parse_proposal(&row.kind, &yaml) {
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
            notify_owner_best_effort(
                state,
                chat_id,
                "Approved, but the artifact could not be re-validated — activation aborted.",
            )
            .await;
            return Ok(());
        }
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

    // Persist to the overlay so activation survives a restart (5d.3):
    // write-to-temp-then-rename, the same atomicity pattern as
    // `ArtifactStore::put`. The registry insert below happens only after a
    // successful rename — a crash in between leaves the overlay file
    // present but the in-memory registry stale until restart, where the
    // startup loader re-merges the overlay; acceptable, not a correctness
    // gap (5d.6).
    let subdir = state.overlay_dir.join(parsed.overlay_subdir());
    std::fs::create_dir_all(&subdir)?;
    let final_path = subdir.join(format!(
        "{}-v{}.yaml",
        parsed.artifact_id(),
        parsed.version()
    ));
    let tmp_path = final_path.with_extension(format!("tmp.{}", row.id));
    std::fs::write(&tmp_path, yaml_text.as_bytes())?;
    std::fs::rename(&tmp_path, &final_path)?;

    let artifact_id = parsed.artifact_id().to_string();
    let version = parsed.version();
    {
        let mut registry = state.registry.write();
        parsed.insert_into(&mut registry);
    }

    state
        .store
        .set_proposed_artifact_state(row.id, Lifecycle::Approved, Lifecycle::Active)?;
    state.store.append_audit(
        "artifact.activated",
        Some(&request.action),
        None,
        None,
        Some(grant.id),
        &[],
        std::slice::from_ref(payload_ref),
    )?;
    notify_owner_best_effort(
        state,
        chat_id,
        &format!("Artifact {artifact_id} v{version} is now active."),
    )
    .await;
    Ok(())
}
