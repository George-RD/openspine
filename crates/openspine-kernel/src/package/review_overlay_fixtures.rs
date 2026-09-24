//! Captured fixture data that startup deliberately does not merge as overlays.
use super::CapturedOverlayState;
use openspine_schemas::digest::{digest_of_bytes, Digest};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct IgnoredOverlayFixture {
    pub kind: &'static str,
    pub artifact_id: String,
    /// Golden sets have no artifact version; the loader's source key uses an
    /// internal sentinel which must never be presented as a fixture version.
    pub version: Option<u32>,
    pub reason: &'static str,
    pub source_digest: Digest,
}

pub(crate) fn assess(
    captured: &CapturedOverlayState,
) -> anyhow::Result<Vec<IgnoredOverlayFixture>> {
    anyhow::ensure!(
        captured.learned.iter().all(|row| row.kind != "golden_set"),
        "fixture-only overlay has learned-artifact controls"
    );
    let mut fixtures = Vec::new();
    for (key, control) in &captured.controls {
        if key.0 != "golden_set" {
            continue;
        }
        anyhow::ensure!(
            key.2 == 1
                && control.lifecycle.is_none()
                && control.highest_active_version.is_none()
                && control.approved_yaml_digest.is_none()
                && !control.recoverable_blob_present
                && control.source_present
                && captured.registry.golden_sets.contains_key(&key.1),
            "fixture-only overlay has proposal controls or invalid identity"
        );
        let source = captured
            .registry
            .sources
            .get(key)
            .ok_or_else(|| anyhow::anyhow!("captured fixture source is missing"))?;
        fixtures.push(IgnoredOverlayFixture {
            kind: "golden_set",
            artifact_id: key.1.clone(),
            version: None,
            reason: "not_loaded_by_runtime",
            source_digest: digest_of_bytes(&source.bytes),
        });
    }
    anyhow::ensure!(
        fixtures.len() == captured.registry.golden_sets.len(),
        "captured fixture controls are missing"
    );
    Ok(fixtures)
}
