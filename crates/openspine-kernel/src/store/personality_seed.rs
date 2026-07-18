//! Kernel bootstrap of the pre-populated Donna×Leo personality seed
//! (AD-080/AD-081/AD-082/AD-083).
//!
//! The seed is NOT kernel-baked: it ships as a set of learnable overlay
//! artifacts, each carrying genuine [`Provenance::ProducedBy`] provenance
//! (D-077) so it enters the registry through the same overlay machinery as
//! any owner-learned artifact — the `Provenance` shape is unchanged from the
//! rest of the codebase; no new provenance kind is introduced here. The
//! producing "exchange" is a real, persisted [`crate::store::Store`] audit
//! event (kind `personality_seed.bootstrap`) minted once per seeding batch,
//! not a bare, disconnected [`Ulid`] — its `id` is what `source_event_id`
//! points at, and its `payload_refs` carry the encrypted exchange
//! description, so both halves of D-077's "non-null producing event
//! identifier and encrypted exchange digest" are genuinely traceable rows,
//! not fabricated values. This is the third activation path — distinct from
//! the propose→approve→activate pipeline (authority-bearing kinds) and from
//! `LegacyMigration` quarantine (discovered overlays).
//!
//! Idempotent and self-healing: a crashed or repeated boot only does the
//! work still outstanding. Three states per element:
//! - DB row present, file present: fully converged, no-op.
//! - DB row present, file missing (e.g. a crash after the durable rename
//!   but whose result was lost, or manual deletion): **repair** — rewrite
//!   the file from the fixed seed content; the existing provenance row is
//!   never touched or re-inserted.
//! - DB row absent: **stage** — write the file, mint (or reuse, within one
//!   `seed_if_missing` call) the batch's bootstrap event, and record a fresh
//!   provenance row.
//!
//! No bootstrap event or exchange artifact is created unless at least one
//! element actually needs staging, so a fully-converged restart performs no
//! audit or artifact-store writes at all.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use jiff::Timestamp;
use openspine_schemas::artifact::{ArtifactNamespace, ArtifactRef, Lifecycle};
use openspine_schemas::digest::{digest_of_bytes, Digest};
use openspine_schemas::persona::PersonaElement;
use ulid::Ulid;

use crate::artifact_loader;
use crate::artifact_store::ArtifactStore;
use crate::store::learned_artifacts::{
    CompatibilityStatus, LearnedArtifact, NominationStatus, Provenance,
};
use crate::store::Store;

const PERSONA_SCHEMA_VERSION: u32 = 1;
const PERSONA_VERSION: u32 = 1;

/// The eight AD-080 Donna×Leo elements plus the AD-082 digest/brief default
/// (AD-135: ship an opinionated default, let corrections converge it). Each
/// is its own overlay artifact so it can diverge independently as the owner
/// corrects behavior.
pub(crate) fn seed_definitions() -> Vec<PersonaElement> {
    vec![
        PersonaElement {
            id: "anticipatory_provisioning".to_string(),
            schema_version: PERSONA_SCHEMA_VERSION,
            version: PERSONA_VERSION,
            lifecycle_state: Lifecycle::Active,
            guidance: "Anticipate what the owner will need next by naming an established pattern and pairing it with a short reason. Prepare drafts, briefs, or context before being asked. Present prepared work as a recommendation for the owner to confirm or adjust. Show the pattern and the receipts, and invite direction.".to_string(),
        },
        PersonaElement {
            id: "bounded_autonomy".to_string(),
            schema_version: PERSONA_SCHEMA_VERSION,
            version: PERSONA_VERSION,
            lifecycle_state: Lifecycle::Active,
            guidance: "Act within the authority already granted for this task. When confidence drops below the level needed to continue safely, escalate to the owner with a crisp recommendation and await direction. Treat the grant boundary as operating context; staying inside it is the work.".to_string(),
        },
        PersonaElement {
            id: "one_loop_confirmation".to_string(),
            schema_version: PERSONA_SCHEMA_VERSION,
            version: PERSONA_VERSION,
            lifecycle_state: Lifecycle::Active,
            guidance: "Resolve a decision in a single exchange. Present your assessment and recommendation, then offer one approve / adjust / decline choice. When genuinely independent decisions are entangled, separate them into clean choices so each can be resolved clearly.".to_string(),
        },
        PersonaElement {
            id: "radical_context_curation".to_string(),
            schema_version: PERSONA_SCHEMA_VERSION,
            version: PERSONA_VERSION,
            lifecycle_state: Lifecycle::Active,
            guidance: "Carry only the context the current task requires. Lead with what changed and why it matters, putting the signal first and making useful detail available behind it. A busy owner reads the first line; earn the rest through relevance.".to_string(),
        },
        PersonaElement {
            id: "discreet_information_discipline".to_string(),
            schema_version: PERSONA_SCHEMA_VERSION,
            version: PERSONA_VERSION,
            lifecycle_state: Lifecycle::Active,
            guidance: "Share exactly the information a request needs in order to be acted on. Hold sensitive detail for the moment it is required, and make the owner's attention a first-class resource. Protecting that attention is a service.".to_string(),
        },
        PersonaElement {
            id: "honest_counsel_with_recommendation".to_string(),
            schema_version: PERSONA_SCHEMA_VERSION,
            version: PERSONA_VERSION,
            lifecycle_state: Lifecycle::Active,
            guidance: "Give your honest assessment, including the options the owner may not want to hear, and close with a clear recommendation. Align with the owner's decision while maintaining independent judgment. State the evidence plainly; the owner chooses.".to_string(),
        },
        PersonaElement {
            id: "provenance_and_receipts".to_string(),
            schema_version: PERSONA_SCHEMA_VERSION,
            version: PERSONA_VERSION,
            lifecycle_state: Lifecycle::Active,
            guidance: "Every action and claim carries a retrievable record of what happened and why: the artifact, the source, and the decision. Make contributions legible to the owner and to any later audit. Frame the work through outcomes and receipts so trust compounds.".to_string(),
        },
        PersonaElement {
            id: "composed_operational_continuity".to_string(),
            schema_version: PERSONA_SCHEMA_VERSION,
            version: PERSONA_VERSION,
            lifecycle_state: Lifecycle::Active,
            guidance: "Treat commitments, open threads, and prior context as first-class state that carries across sessions. Resume where the last exchange left off and reference what was decided. Continuity is composed, deliberate, and easy to follow.".to_string(),
        },
        PersonaElement {
            id: "digest_brief_default".to_string(),
            schema_version: PERSONA_SCHEMA_VERSION,
            version: PERSONA_VERSION,
            lifecycle_state: Lifecycle::Active,
            guidance: "Present a concise digest with at most three priority items, sorted by decisions needed, then FYI, then handled. Keep each item to one clear line and make supporting detail available on request. Favor fewer items and sharper lines.".to_string(),
        },
    ]
}

fn persona_overlay_dir(overlay_dir: &Path) -> PathBuf {
    overlay_dir.join("personas")
}

/// Durably write one persona element's YAML into the overlay: temp file →
/// `fsync` the file → rename → `fsync` the containing directory. A rename is
/// not durable on its own (POSIX): the directory entry recording it can
/// still be lost on power loss until the directory itself is `fsync`ed, so
/// both syncs are required before the file can be trusted to survive a
/// crash. Returns the published path and the content digest.
fn write_persona_file(
    overlay_dir: &Path,
    element: &PersonaElement,
) -> anyhow::Result<(PathBuf, Digest)> {
    let yaml = serde_yaml::to_string(element).context("serializing persona seed element")?;
    let bytes = yaml.into_bytes();
    let digest = digest_of_bytes(&bytes);
    let personas_dir = persona_overlay_dir(overlay_dir);
    std::fs::create_dir_all(&personas_dir).context("creating persona overlay dir")?;
    let final_path = personas_dir.join(artifact_loader::overlay_filename(
        &element.id,
        element.version,
    ));
    let tmp_name = format!(
        "{}.tmp.{}",
        final_path
            .file_name()
            .expect("persona file name")
            .to_string_lossy(),
        Ulid::new()
    );
    let tmp_path = personas_dir.join(tmp_name);
    {
        let mut file =
            std::fs::File::create(&tmp_path).context("creating persona seed temp file")?;
        file.write_all(&bytes)
            .context("writing persona seed temp file")?;
        file.sync_all().context("fsyncing persona seed temp file")?;
    }
    std::fs::rename(&tmp_path, &final_path).context("publishing persona seed file")?;
    let dir_handle =
        std::fs::File::open(&personas_dir).context("opening persona overlay dir for fsync")?;
    dir_handle
        .sync_all()
        .context("fsyncing persona overlay dir after rename")?;
    Ok((final_path, digest))
}

/// Full staging of a brand-new persona element: durable file write, then a
/// fresh provenance row bound to the batch's real bootstrap event.
fn stage_persona(
    store: &Store,
    overlay_dir: &Path,
    element: &PersonaElement,
    source_event_id: Ulid,
    exchange_ref: &ArtifactRef,
) -> anyhow::Result<()> {
    let (final_path, digest) = write_persona_file(overlay_dir, element)?;
    let row = LearnedArtifact {
        kind: "persona".to_string(),
        artifact_id: element.id.clone(),
        version: element.version,
        namespace: ArtifactNamespace::Overlay,
        provenance: Provenance::ProducedBy {
            source_event_id,
            source_exchange: exchange_ref.clone(),
        },
        accepted_via: None,
        learned_at: Timestamp::now(),
        compatibility: CompatibilityStatus::Compatible,
        nomination: NominationStatus::None,
        pending_reconfirmation_id: None,
        pending_yaml_digest: Some(digest.to_string()),
        accepted_dependency_fingerprint: None,
        source_path: Some(final_path.to_string_lossy().into_owned()),
        accepted_base_epoch: None,
    };
    store
        .record_learned_artifact_with_audit(
            &row,
            "personality_seed.seeded",
            &format!(
                "persona element {} v{} seeded as learnable overlay artifact (AD-080): \
                 producing event {}; exchange {}",
                element.id, element.version, source_event_id, exchange_ref.digest
            ),
        )
        .context("recording persona seed provenance and receipt")?;
    Ok(())
}

fn valid_seed_provenance(
    store: &Store,
    artifacts: &ArtifactStore,
    row: &LearnedArtifact,
) -> anyhow::Result<bool> {
    let Provenance::ProducedBy {
        source_event_id,
        source_exchange,
    } = &row.provenance
    else {
        return Ok(false);
    };
    let Some(event) = store
        .validated_audit_event_by_id(*source_event_id)
        .context("validating personality seed provenance event")?
    else {
        return Ok(false);
    };
    Ok(event
        .payload_refs
        .iter()
        .any(|reference| reference == source_exchange)
        && artifacts.get(source_exchange).is_ok())
}

/// Idempotently seed the Donna×Leo personality elements into the overlay.
/// Safe to call on every boot: a row with valid bootstrap provenance is kept
/// and its missing/corrupt file is repaired from the recorded digest. Rows
/// with invalid provenance or a foreign digest are quarantined, then treated
/// as absent so the canonical element is reseeded with fresh provenance.
pub(crate) fn seed_if_missing(
    store: &Store,
    artifacts: &ArtifactStore,
    overlay_dir: &Path,
) -> anyhow::Result<()> {
    let learned = store
        .list_learned_artifacts()
        .context("loading learned artifact provenance for personality seed")?;
    let definitions = seed_definitions();
    let mut valid_rows = std::collections::HashMap::new();
    for element in &definitions {
        let canonical_bytes = serde_yaml::to_string(element)
            .context("serializing canonical personality seed element")?
            .into_bytes();
        let canonical_digest = digest_of_bytes(&canonical_bytes).to_string();
        for row in learned
            .iter()
            .filter(|item| item.kind == "persona" && item.artifact_id == element.id)
        {
            let valid = row.version == element.version
                && row.pending_yaml_digest.as_deref() == Some(canonical_digest.as_str())
                && valid_seed_provenance(store, artifacts, row)?;
            if valid {
                valid_rows.insert((row.artifact_id.as_str(), row.version), row);
            } else {
                store
                    .quarantine_learned_artifact(
                        &row.kind,
                        &row.artifact_id,
                        row.version,
                        "canonical personality seed row failed provenance or digest validation",
                    )
                    .with_context(|| {
                        format!(
                            "quarantining invalid personality seed row {} v{}",
                            row.artifact_id, row.version
                        )
                    })?;
            }
        }
    }

    let personas_dir = persona_overlay_dir(overlay_dir);
    let mut needs_stage = Vec::new();
    let mut needs_repair = Vec::new();
    for element in definitions {
        let expected_path = personas_dir.join(artifact_loader::overlay_filename(
            &element.id,
            element.version,
        ));
        let recorded = valid_rows
            .get(&(element.id.as_str(), element.version))
            .copied();
        // A row exists: verify the on-disk file is present AND reproduces the
        // row's recorded digest. Missing or mismatched bytes are corruption
        // signals (personas have no learned-correction pathway in this
        // change), so they are repaired from the row's own digest.
        if let Some(row) = recorded {
            // A row exists: verify the on-disk file is present AND reproduces
            // the row's recorded digest. Manifest `NotFound` means the file
            // was lost (repairable); any other read error is a real I/O fault
            // and is propagated rather than silently masked as corruption; a
            // present-but-mismatched file is a corruption signal. Personas
            // have no learned-correction pathway in this change, so any of
            // these repair from the row's own digest.
            let on_disk_digest = match std::fs::read(&expected_path) {
                Ok(bytes) => Some(digest_of_bytes(&bytes).to_string()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
                Err(err) => {
                    return Err(err).with_context(|| {
                        format!("reading persona file {}", expected_path.display())
                    })
                }
            };
            if matches!(
                (on_disk_digest.as_deref(), row.pending_yaml_digest.as_deref()),
                (Some(actual), Some(expected)) if actual == expected
            ) {
                continue;
            }
            needs_repair.push((element, row));
            continue;
        }
        // No valid row: either the element is new or an invalid row was
        // quarantined above; stage the canonical element.
        needs_stage.push(element);
    }

    // Repair never mints new provenance or touches the DB row — it only
    // restores a missing or corrupt-on-disk file for a row that already
    // legitimately exists. A mismatch between the on-disk bytes and the
    // row's recorded `pending_yaml_digest` is a corruption signal (not a
    // legitimately-evolved artifact — personas have no correction pathway
    // in this change), so repair is fail-closed: it refuses to publish
    // anything that does not reproduce the row's own digest.
    for (element, row) in &needs_repair {
        repair_persona_file(overlay_dir, element, row)?;
    }

    if needs_stage.is_empty() {
        return Ok(());
    }

    // Only now — at least one element needs staging — mint the batch's real
    // bootstrap event and its encrypted exchange payload, so a fully-converged
    // restart never creates an unused audit/artifact-store row.
    let exchange_ref = artifacts
        .put(
            b"openspine personality seed bootstrap: kernel-authored exchange \
establishing ProducedBy provenance for the pre-populated Donna x Leo persona \
overlay artifacts (AD-080). Not a human conversation; a traceable bootstrap event.",
        )
        .context("storing persona seed bootstrap exchange")?;
    let bootstrap_event = store
        .append_audit(
            "personality_seed.bootstrap",
            None,
            None,
            Some("kernel-authored personality seed bootstrap (AD-080)"),
            None,
            &[],
            std::slice::from_ref(&exchange_ref),
        )
        .context("recording persona seed bootstrap event")?;

    for element in &needs_stage {
        // Shipped defaults must not violate their own anti-patterns
        // (AD-081/AD-083): a probe hit here means the seed content is wrong,
        // not the model output.
        let violations =
            crate::overlay_eval_gate::personality_probes::run_probes(&element.guidance);
        if !violations.is_empty() {
            anyhow::bail!(
                "persona seed element {} violates anti-pattern probe(s): {:?}",
                element.id,
                violations
                    .iter()
                    .map(|v| v.anti_pattern.description())
                    .collect::<Vec<_>>()
            );
        }
        stage_persona(
            store,
            overlay_dir,
            element,
            bootstrap_event.id,
            &exchange_ref,
        )?;
    }
    Ok(())
}

/// Restore the on-disk YAML for a persona whose `learned_artifacts` row
/// already exists but whose file is missing or corrupt on disk (e.g. a crash
/// after the durable rename, manual deletion, or bit-rot). The file is
/// regenerated from [`seed_definitions`] and only published if it reproduces
/// the row's own `pending_yaml_digest` — never a divergent value. If today's
/// seed can no longer reproduce that digest, the row is left untouched and
/// the call fails loudly rather than silently overwriting with defaults.
fn repair_persona_file(
    overlay_dir: &Path,
    element: &PersonaElement,
    row: &LearnedArtifact,
) -> anyhow::Result<()> {
    let expected = row.pending_yaml_digest.clone().ok_or_else(|| {
        anyhow::anyhow!(
            "persona {} v{} row has no recorded pending_yaml_digest; refusing repair",
            element.id,
            element.version
        )
    })?;
    let yaml = serde_yaml::to_string(element).context("serializing persona seed element")?;
    let digest = digest_of_bytes(&yaml.into_bytes());
    if digest.to_string() != expected {
        anyhow::bail!(
            "persona {} v{} cannot be repaired: current seed digest {} does not \
             match the recorded row digest {}; refusing to overwrite (AD-080)",
            element.id,
            element.version,
            digest,
            expected
        );
    }
    write_persona_file(overlay_dir, element)?;
    Ok(())
}

#[cfg(test)]
#[path = "personality_seed_tests.rs"]
mod tests;
