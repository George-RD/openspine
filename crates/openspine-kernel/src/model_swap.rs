//! AD-152 model-swap runtime mechanics.
//!
//! This module owns the only constructor for enriched golden-set evidence:
//! it resolves a kernel-owned [`GoldenSet`], calls the candidate provider for
//! every bounded case, and derives deterministic criterion verdicts from the
//! observed output. The generic synchronous overlay gate refuses model-swap
//! proposals; the proposal dispatcher must call [`enrich`] first, then use
//! the enriched manifest as the digest-bound artifact bytes.

use std::time::Duration;

use crate::model_gateway::{PromptMessage, PromptRole, ProviderClient, ResolvedPrompt};
use crate::pipeline::AppState;
use crate::spend::{counted_model_generate, SpendLane, SpendModelError};
use openspine_schemas::digest::{digest_of, digest_of_bytes, Digest};
use openspine_schemas::model_swap::{
    GoldenSet, GoldenSetCase, GoldenSetCaseResult, GoldenSetRunResult, ModelSwapManifest,
    MAX_OBSERVED_EXCERPT_BYTES,
};
use serde_json::to_value;

const GOLDEN_SET_TIMEOUT: Duration = Duration::from_secs(300);
const GOLDEN_SET_MAX_TOKENS: u32 = 512;

fn remaining_runtime(
    expires_at: jiff::Timestamp,
    now: jiff::Timestamp,
) -> anyhow::Result<Duration> {
    let remaining = expires_at
        .since(now)
        .map_err(|err| anyhow::anyhow!("invalid grant expiry: {err}"))?;
    std::time::Duration::try_from(remaining)
        .map_err(|_| anyhow::anyhow!("grant runtime is exhausted"))
}

/// Compute the canonical digest of the trusted golden-set content. Both
/// proposal-time evidence and activation-time drift checks call this helper;
/// it never hashes YAML formatting or filesystem bytes.
pub fn golden_set_digest(golden_set: &GoldenSet) -> Digest {
    digest_of(&to_value(golden_set).expect("GoldenSet serialization cannot fail"))
}

/// Run the trusted golden set against the already-resolved candidate provider,
/// deriving every case result from the provider's actual output. The caller
pub async fn enrich(
    state: &AppState,
    manifest: &ModelSwapManifest,
    golden_set: &GoldenSet,
    provider: &ProviderClient,
    provider_config_digest: &Digest,
    grant_expires_at: jiff::Timestamp,
) -> anyhow::Result<ModelSwapManifest> {
    golden_set.validate()?;
    if manifest.golden_set_result.is_some() {
        anyhow::bail!("model swap must not carry proposer-supplied golden_set_result");
    }
    let remaining = remaining_runtime(grant_expires_at, jiff::Timestamp::now())?;
    let timeout = remaining.min(GOLDEN_SET_TIMEOUT);
    let set_digest = golden_set_digest(golden_set);
    let cases = tokio::time::timeout(timeout, run_cases(state, golden_set, provider))
        .await
        .map_err(|_| anyhow::anyhow!("golden-set execution exceeded grant runtime"))??;
    let mut enriched = manifest.clone();
    enriched.golden_set_result = Some(GoldenSetRunResult {
        golden_set_id: golden_set.id.clone(),
        golden_set_digest: set_digest.to_string(),
        provider_config_digest: provider_config_digest.to_string(),
        cases,
    });
    Ok(enriched)
}

async fn run_cases(
    state: &AppState,
    golden_set: &GoldenSet,
    provider: &ProviderClient,
) -> anyhow::Result<Vec<GoldenSetCaseResult>> {
    let mut results = Vec::with_capacity(golden_set.cases.len());
    for case in &golden_set.cases {
        results.push(run_case(state, golden_set, case, provider).await?);
    }
    Ok(results)
}
async fn run_case(
    state: &AppState,
    golden_set: &GoldenSet,
    case: &GoldenSetCase,
    provider: &ProviderClient,
) -> anyhow::Result<GoldenSetCaseResult> {
    let prompt = ResolvedPrompt {
        system: golden_set.system.clone().unwrap_or_default(),
        messages: vec![PromptMessage {
            role: PromptRole::User,
            content: case.prompt.clone(),
        }],
        max_tokens: GOLDEN_SET_MAX_TOKENS,
    };
    let output = counted_model_generate(state, SpendLane::NonImmediate, provider, &prompt)
        .await
        .map_err(|err| match err {
            SpendModelError::Provider(provider_err) => anyhow::anyhow!(provider_err),
            SpendModelError::Ledger(store_err) => anyhow::anyhow!(store_err),
            SpendModelError::Denied => anyhow::anyhow!("daily model spend cap exceeded"),
        })?;
    let passed = case
        .must_contain
        .iter()
        .all(|needle| output.contains(needle))
        && case
            .must_not_contain
            .iter()
            .all(|needle| !output.contains(needle));
    Ok(GoldenSetCaseResult {
        case_id: case.id.clone(),
        kind: case.kind,
        passed,
        observed_excerpt: bounded_excerpt(&output),
        observed_digest: digest_of_bytes(output.as_bytes()).to_string(),
    })
}

fn bounded_excerpt(output: &str) -> String {
    if output.len() <= MAX_OBSERVED_EXCERPT_BYTES {
        return output.to_string();
    }
    let end = output
        .char_indices()
        .take_while(|(index, ch)| *index + ch.len_utf8() <= MAX_OBSERVED_EXCERPT_BYTES)
        .map(|(index, ch)| index + ch.len_utf8())
        .last()
        .unwrap_or(0);
    output[..end].to_string()
}

/// Activation-time binding check. It must run before any lifecycle, overlay,
/// registry, or active-provider mutation. A changed trusted golden set or
/// changed non-secret provider configuration therefore aborts activation
/// rather than silently applying stale evidence.
pub fn verify_activation_binding(
    manifest: &ModelSwapManifest,
    golden_set: &GoldenSet,
    provider_config_digest: &Digest,
) -> anyhow::Result<()> {
    let result = manifest
        .golden_set_result
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("model swap has no kernel-verified golden-set result"))?;
    if result.golden_set_id != golden_set.id {
        anyhow::bail!("golden-set id changed between proposal and activation");
    }
    if result.golden_set_digest != golden_set_digest(golden_set).as_str() {
        anyhow::bail!("golden-set digest changed between proposal and activation");
    }
    if result.provider_config_digest != provider_config_digest.as_str() {
        anyhow::bail!("candidate provider configuration changed between proposal and activation");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_excerpt_never_exceeds_byte_cap_at_utf8_boundary() {
        let output = format!("{}🙂", "a".repeat(MAX_OBSERVED_EXCERPT_BYTES - 1));
        let excerpt = bounded_excerpt(&output);
        assert!(excerpt.len() <= MAX_OBSERVED_EXCERPT_BYTES);
        assert!(excerpt.is_char_boundary(excerpt.len()));
    }

    #[test]
    fn remaining_runtime_respects_short_grant_and_rejects_expiry() {
        let now = jiff::Timestamp::now();
        let short = remaining_runtime(now + std::time::Duration::from_secs(60), now).unwrap();
        assert_eq!(short, Duration::from_secs(60));
        assert!(remaining_runtime(now - std::time::Duration::from_secs(1), now).is_err());
    }
}
