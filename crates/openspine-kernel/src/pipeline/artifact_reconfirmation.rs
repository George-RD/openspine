//! Post-approval handler for `artifact.reconfirm` (AD-070). Restores a
//! learned overlay artifact to the effective registry only after the owner's
//! single-use, digest-bound tap re-derives the exact YAML bytes stored with
//! the pending review row.

use jiff::Timestamp;
use openspine_schemas::action::ActionRequest;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::digest::digest_of_bytes;
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::lineage::ArtifactLineage;
use ulid::Ulid;

use super::{notify_owner_best_effort, AppState};
use crate::store::learned_artifacts::Provenance;
use crate::store::proposed_artifacts::ProposedArtifact;

pub(super) async fn reinstate_artifact(
    state: &AppState,
    grant: &TaskGrant,
    request: &ActionRequest,
    chat_id: i64,
) -> anyhow::Result<()> {
    let Some(payload_ref) = request.payload_ref.as_ref() else {
        state.store.append_audit(
            "artifact.reconfirm_malformed",
            Some(&request.action),
            None,
            Some("reconfirm request has no payload ref"),
            Some(grant.id),
            &[],
            &[],
        )?;
        return Ok(());
    };
    let bytes = state.artifacts.get(payload_ref)?;
    let yaml = String::from_utf8_lossy(&bytes);

    let learned = state.store.list_learned_artifacts()?;
    let Some(row) = learned
        .iter()
        .find(|item| item.pending_reconfirmation_id == Some(request.id))
        .cloned()
    else {
        state.store.append_audit(
            "artifact.reconfirm_no_pending",
            Some(&request.action),
            None,
            Some("no learned artifact pending this reconfirmation id"),
            Some(grant.id),
            &[],
            &[],
        )?;
        notify_owner_best_effort(state, chat_id, "That reconfirmation is no longer valid.").await;
        return Ok(());
    };
    let current_path = row
        .source_path
        .as_deref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            let subdir = crate::artifact_loader::overlay_subdir_for_kind(&row.kind).unwrap_or("");
            state
                .overlay_dir
                .join(subdir)
                .join(crate::artifact_loader::overlay_filename(
                    &row.artifact_id,
                    row.version,
                ))
        });
    let current_bytes = match std::fs::read(&current_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            state.store.append_audit(
                "artifact.reconfirm_disk_missing",
                Some(&request.action),
                None,
                Some("overlay file is absent at tap time; re-propose"),
                Some(grant.id),
                &[],
                &[],
            )?;
            tracing::warn!(error = %err, path = %current_path.display(), "reconfirm overlay file missing");
            return Ok(());
        }
    };
    if current_bytes != bytes {
        state.store.append_audit(
            "artifact.reconfirm_disk_changed",
            Some(&request.action),
            None,
            Some("overlay file changed after review; re-propose"),
            Some(grant.id),
            &[],
            std::slice::from_ref(payload_ref),
        )?;
        notify_owner_best_effort(
            state,
            chat_id,
            "The overlay changed after review — please re-propose it.",
        )
        .await;
        return Ok(());
    }

    let derived = digest_of_bytes(&bytes).as_str().to_string();
    if row.pending_yaml_digest.as_deref() != Some(derived.as_str()) {
        state.store.append_audit(
            "artifact.reconfirm_digest_mismatch",
            Some(&request.action),
            None,
            Some("overlay bytes changed since review; re-propose"),
            Some(grant.id),
            &[],
            std::slice::from_ref(payload_ref),
        )?;
        notify_owner_best_effort(
            state,
            chat_id,
            "The overlay changed since it was flagged — please re-propose it.",
        )
        .await;
        return Ok(());
    }

    if state
        .base_artifact_ids
        .contains(&(row.kind.clone(), row.artifact_id.clone()))
    {
        state.store.append_audit(
            "artifact.reconfirm_namespace_collision",
            Some(&request.action),
            None,
            Some("overlay identity collides with base; choose a new id"),
            Some(grant.id),
            &[],
            std::slice::from_ref(payload_ref),
        )?;
        notify_owner_best_effort(
            state,
            chat_id,
            "Re-confirmation refused — choose a new identity instead of replacing base.",
        )
        .await;
        return Ok(());
    }
    // Trusted reconfirm parser. Templates are never chat-proposable (D-048)
    // but an owner may explicitly re-confirm a discovered legacy template
    // file, so we parse `PromptTemplate` directly here; every other kind uses
    // the normal proposal parser. Both yield the post-activation candidate
    // registry + superseded version + dangling references for uniform
    // publication.
    let (candidate, superseded, dangling) = if row.kind == "template" {
        let mut template: crate::model_gateway::PromptTemplate = serde_yaml::from_str(&yaml)
            .map_err(|err| {
                let _ = state.store.append_audit(
                    "artifact.reconfirm_reparse_failed",
                    Some(&request.action),
                    None,
                    Some("reviewed template YAML failed to parse"),
                    Some(grant.id),
                    &[],
                    &[],
                );
                tracing::warn!(error = %err, "reconfirm template YAML failed to parse");
                anyhow::anyhow!("template reparse failed")
            })?;
        if template.id != row.artifact_id || template.version != row.version {
            state.store.append_audit(
                "artifact.reconfirm_identity_mismatch",
                Some(&request.action),
                None,
                Some("reviewed identity does not match pending row"),
                Some(grant.id),
                &[],
                &[],
            )?;
            return Ok(());
        }
        if crate::artifact_loader::artifact_version(
            &state.registry.read(),
            &row.kind,
            &row.artifact_id,
        )
        .is_some_and(|current| current > row.version)
        {
            state.store.append_audit(
                "artifact.reconfirm_stale_version",
                Some(&request.action),
                None,
                Some("pending reconfirmation is superseded by a higher active version"),
                Some(grant.id),
                &[],
                std::slice::from_ref(payload_ref),
            )?;
            return Ok(());
        }
        template.lifecycle_state = Lifecycle::Active;
        let old = state
            .registry
            .read()
            .templates
            .get(&template.id)
            .map(|existing| existing.version)
            .filter(|version| *version < template.version);
        let mut cand = state.registry.read().clone();
        cand.templates.insert(template.id.clone(), template);
        (cand, old, vec![])
    } else {
        let mut parsed = match crate::artifact_loader::parse_proposal(&row.kind, &yaml) {
            Ok(parsed) => parsed,
            Err(err) => {
                state.store.append_audit(
                    "artifact.reconfirm_reparse_failed",
                    Some(&request.action),
                    None,
                    Some("reviewed YAML failed to re-parse"),
                    Some(grant.id),
                    &[],
                    &[],
                )?;
                tracing::warn!(error = %err, "reconfirm YAML failed to re-parse");
                return Ok(());
            }
        };
        if parsed.artifact_id() != row.artifact_id || parsed.version() != row.version {
            state.store.append_audit(
                "artifact.reconfirm_identity_mismatch",
                Some(&request.action),
                None,
                Some("reviewed identity does not match pending row"),
                Some(grant.id),
                &[],
                &[],
            )?;
            return Ok(());
        }
        if crate::artifact_loader::artifact_version(
            &state.registry.read(),
            &row.kind,
            &row.artifact_id,
        )
        .is_some_and(|current| current > row.version)
        {
            state.store.append_audit(
                "artifact.reconfirm_stale_version",
                Some(&request.action),
                None,
                Some("pending reconfirmation is superseded by a higher active version"),
                Some(grant.id),
                &[],
                std::slice::from_ref(payload_ref),
            )?;
            return Ok(());
        }
        let dangling = {
            let registry = state.registry.read();
            crate::overlay_compat::dangling_for_parsed(&registry, &parsed)
        };
        parsed.activate();
        let preflight = {
            let registry = state.registry.read();
            let mut candidate = (*registry).clone();
            parsed
                .insert_into(&mut candidate)
                .map(|superseded| (candidate, superseded, dangling))
        };
        match preflight {
            Ok(result) => result,
            Err(err) => {
                state.store.append_audit(
                    "artifact.reconfirm_version_conflict",
                    Some(&request.action),
                    None,
                    Some(&format!("version conflict: {err}")),
                    Some(grant.id),
                    &[],
                    &[],
                )?;
                notify_owner_best_effort(
                    state,
                    chat_id,
                    "Re-confirmation version conflict — re-propose with a higher version.",
                )
                .await;
                return Ok(());
            }
        }
    };
    // Persist the durable disposition and consumed-request anchor inside a
    // single store transaction, THEN publish to the live registry only on
    // success. Reviewed dangling references are explicitly owner-accepted;
    // digest and base-collision checks above still gate this mutation.
    let accepted_via = crate::store::learned_artifacts::ReconfirmAnchor {
        request_id: request.id,
        grant_event_id: grant.event_id,
        reviewed_ref: payload_ref.clone(),
    };
    // `row.provenance` is the original producing provenance. A LegacyMigration
    // row was only a quarantine placeholder: the owner's tap establishes a
    // fresh `ProducedBy` exchange link (this grant's event id + the reviewed
    // bytes' digest) BEFORE any visibility, so LegacyMigration provenance is
    // never published. An already-`ProducedBy` row keeps its link untouched.
    let effective_provenance = match &row.provenance {
        crate::store::learned_artifacts::Provenance::LegacyMigration { .. } => {
            crate::store::learned_artifacts::Provenance::ProducedBy {
                source_event_id: grant.event_id,
                source_exchange: payload_ref.clone(),
            }
        }
        other => other.clone(),
    };
    // The matching `Approved` proposal (if any) advances to `Active` inside
    // the same transaction, via affected-row count — a legacy/no-proposal
    // Resolve the proposal lifecycle this reconfirm completes. A LegacyMigration
    // tap (quarantined file) has no prior proposal: mint a fresh digest-bound
    // `Approved` proposal bound to the reviewed bytes and let the reconfirm
    // transaction flip it `Approved -> Active` — the normal artifact lifecycle,
    // never an activation under LegacyMigration provenance. An already-proposed
    // artifact reuses its existing `Approved` row. Both paths are derived from
    // affected-row counts inside the transaction.
    let (new_proposal, proposal_id) = match &row.provenance {
        Provenance::LegacyMigration { .. } => {
            let id = Ulid::new();
            let proposal = ProposedArtifact {
                id,
                kind: row.kind.clone(),
                artifact_id: row.artifact_id.clone(),
                version: row.version,
                state: Lifecycle::Proposed,
                yaml_digest: payload_ref.digest.to_string(),
                task_grant_id: grant.id,
                action_request_id: Some(request.id),
                proposed_at: Timestamp::now(),
                lineage: Some(ArtifactLineage::root()),
            };
            (Some(proposal), Some(id))
        }
        _ => {
            let proposed =
                state
                    .store
                    .find_proposed_artifact(&row.kind, &row.artifact_id, row.version)?;
            let Some(proposed) = proposed else {
                return Err(anyhow::anyhow!(
                    "reconfirmation requires a matching proposal lifecycle"
                ));
            };
            if proposed.state == Lifecycle::Active {
                state.store.set_proposed_artifact_state(
                    proposed.id,
                    Lifecycle::Active,
                    Lifecycle::Approved,
                )?;
                state.store.append_audit(
                    "artifact.reconfirmation_reopened",
                    Some(&request.action),
                    None,
                    Some("active proposal reopened for owner reconfirmation"),
                    Some(grant.id),
                    &[],
                    &[],
                )?;
            }
            (None, Some(proposed.id))
        }
    };
    let committed = state.store.commit_owner_reconfirmation(
        crate::store::learned_reconfirmation::OwnerReconfirmation {
            kind: row.kind.clone(),
            artifact_id: row.artifact_id.clone(),
            version: row.version,
            provenance: effective_provenance,
            accepted_via: Some(accepted_via),
            base_epoch: state.base_compatibility_epoch.clone(),
            accepted_dependency_fingerprint: Some(
                crate::store::learned_artifacts::dependency_fingerprint(&dangling),
            ),
            request_id: request.id,
            grant_id: Some(grant.id),
            review_ref: Some(payload_ref.clone()),
            proposal_id,
            new_proposal,
            dangling_refs: dangling.clone(),
            superseded_old_version: superseded,
        },
    )?;
    if !committed {
        // Lost a race to a concurrent/duplicate tap (already handled): the
        // winning tap published. Leave the registry as-is and say nothing.
        return Ok(());
    }
    // Durable commit succeeded (every acceptance/activation/dangling/
    // superseded audit is inside that transaction); publish to the live
    // registry now. No await or fallible DB write follows publication.
    {
        let mut registry = state.registry.write();
        *registry = candidate;
    }
    notify_owner_best_effort(
        state,
        chat_id,
        &format!("Artifact {} v{} restored.", row.artifact_id, row.version),
    )
    .await;
    Ok(())
}
