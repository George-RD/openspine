//! Provenance-bound admission for learnable persona overlays.

use anyhow::Context as _;
use openspine_schemas::artifact::ArtifactRef;
use std::collections::BTreeMap;
use std::path::Path;

use crate::artifact_loader::{self, ArtifactRegistry};
use crate::artifact_store::ArtifactStore;
use crate::store::learned_artifacts::{CompatibilityStatus, LearnedArtifact, Provenance};
use crate::store::Store;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PersonaProvenanceExclusion {
    Erased,
    NotProducedBy,
    EventUnavailable,
    ExchangeNotBound,
    ExchangeUnavailable,
    DigestMissing,
}

/// Provenance findings, not complete persona admission. YAML integrity,
/// lifecycle, and highest-active-version checks still belong to their phases.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PersonaProvenanceFindings {
    pub expected_digests: BTreeMap<(String, u32), String>,
    pub excluded: BTreeMap<(String, u32), PersonaProvenanceExclusion>,
}

/// Owned observations from one provenance capture; never retains plaintext
/// exchange content or handles to live storage.
pub(crate) struct CapturedPersonaProvenance {
    evidence: Vec<PersonaEvidence>,
}

struct PersonaEvidence {
    row: LearnedArtifact,
    event_payload_refs: Option<Vec<ArtifactRef>>,
    exchange_readable: bool,
}

impl CapturedPersonaProvenance {
    /// Capture through the existing startup readers. The caller must exclude
    /// concurrent writers and supply consistent learned rows. This is not an
    /// exhaustive overlay snapshot or an atomic cross-store transaction.
    ///
    /// This adapter is intentionally startup-only: ArtifactStore reads can
    /// recover legacy key/blob formats. A read-only package review needs a
    /// non-mutating capture adapter before it can use the same evaluator.
    pub(crate) fn capture_for_startup(
        store: &Store,
        artifacts: &ArtifactStore,
        learned: &[LearnedArtifact],
    ) -> anyhow::Result<Self> {
        let mut rows = BTreeMap::new();
        // Check every persona identity, including erased rows, before any
        // reads. Ambiguous evidence must never become last-writer-wins.
        for row in learned.iter().filter(|row| row.kind == "persona") {
            let key = (row.artifact_id.clone(), row.version);
            if rows.insert(key, row).is_some() {
                anyhow::bail!(
                    "duplicate persona provenance for {} v{}",
                    row.artifact_id,
                    row.version
                );
            }
        }
        let evidence = rows
            .into_values()
            .map(|row| PersonaEvidence::capture(store, artifacts, row))
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self { evidence })
    }

    /// Deterministic and side-effect free. No storage reads, generated IDs,
    /// audit writes, recovery, or reconfirmation occur during evaluation.
    pub(crate) fn evaluate(&self) -> PersonaProvenanceFindings {
        let mut findings = PersonaProvenanceFindings {
            expected_digests: BTreeMap::new(),
            excluded: BTreeMap::new(),
        };
        for evidence in &self.evidence {
            let key = (evidence.row.artifact_id.clone(), evidence.row.version);
            match evidence.expected_digest() {
                Ok(digest) => {
                    findings.expected_digests.insert(key, digest.to_string());
                }
                Err(reason) => {
                    findings.excluded.insert(key, reason);
                }
            }
        }
        findings
    }
}

impl PersonaEvidence {
    fn capture(
        store: &Store,
        artifacts: &ArtifactStore,
        row: &LearnedArtifact,
    ) -> anyhow::Result<Self> {
        let mut evidence = Self {
            row: row.clone(),
            event_payload_refs: None,
            exchange_readable: false,
        };
        if row.compatibility == CompatibilityStatus::Erased {
            return Ok(evidence);
        }
        if let Provenance::ProducedBy {
            source_event_id,
            source_exchange,
            source_scope,
        } = &row.provenance
        {
            let event = store
                .validated_audit_event_by_id(*source_event_id)
                .with_context(|| {
                    format!(
                        "resolving persona {} v{} provenance event",
                        row.artifact_id, row.version
                    )
                })?;
            if let Some(event) = event {
                // Preserve startup's short circuit: an unbound event never
                // causes an exchange read, including a legacy-format repair.
                if event.payload_refs.contains(source_exchange) {
                    evidence.exchange_readable = artifacts
                        .get_scoped(source_scope.producing_scope(), source_exchange)
                        .is_ok();
                }
                evidence.event_payload_refs = Some(event.payload_refs);
            }
        }
        Ok(evidence)
    }

    fn expected_digest(&self) -> Result<&str, PersonaProvenanceExclusion> {
        use PersonaProvenanceExclusion as Excluded;

        if self.row.compatibility == CompatibilityStatus::Erased {
            return Err(Excluded::Erased);
        }
        let Provenance::ProducedBy {
            source_exchange, ..
        } = &self.row.provenance
        else {
            return Err(Excluded::NotProducedBy);
        };
        let refs = self
            .event_payload_refs
            .as_ref()
            .ok_or(Excluded::EventUnavailable)?;
        if !refs.contains(source_exchange) {
            return Err(Excluded::ExchangeNotBound);
        }
        if !self.exchange_readable {
            return Err(Excluded::ExchangeUnavailable);
        }
        self.row
            .pending_yaml_digest
            .as_deref()
            .ok_or(Excluded::DigestMissing)
    }
}

/// Admit only persona YAML whose learned row, validated ledger event, exchange,
/// and digest all agree. Invalid rows are quarantined from this boot.
pub(crate) fn admit(
    store: &Store,
    artifacts: &ArtifactStore,
    overlay_dir: &Path,
    learned: &[LearnedArtifact],
    registry: &mut ArtifactRegistry,
) -> anyhow::Result<()> {
    let findings = CapturedPersonaProvenance::capture_for_startup(store, artifacts, learned)?
        .evaluate();
    for ((id, version), reason) in findings.excluded {
        if reason != PersonaProvenanceExclusion::Erased {
            tracing::warn!(artifact_id = %id, version, ?reason,
                "excluding persona with unavailable provenance backing");
        }
    }
    let admitted = findings.expected_digests.into_iter().collect();
    artifact_loader::load_admitted_personas(registry, overlay_dir, &admitted)
        .context("admitting provenance-backed persona overlays")
}

#[cfg(test)]
#[path = "overlay_persona_admission_tests.rs"]
mod tests;
