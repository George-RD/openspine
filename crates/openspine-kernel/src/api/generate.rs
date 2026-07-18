//! `POST /v1/model/generate` — kernel-mediated model invocation (build plan
//! 4a/4c/4d). Internally gated as `model.generate:approved_provider`
//! before any provider call.

use std::sync::Arc;

use crate::spend::SpendLane;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use jiff::Timestamp;
use openspine_gate::{gate, ActionOrigin};
use openspine_schemas::action::{ActionId, ActionRequest, DenialReason, GateDecision};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::canonical_json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ulid::Ulid;

use super::{authenticate, internal_error};
use crate::model_gateway::{
    build_prompt, build_prompt_with_untrusted_context, PromptMessage, PromptRole,
};
use crate::pipeline::AppState;
use crate::spend::{counted_model_generate, SpendModelError};
use openspine_schemas::workflow::ReasoningTier;

const CONVERSATION_HISTORY_LIMIT: usize = 20;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GenerateRequestBody {
    #[allow(dead_code)] // carried for audit/future routing; not yet branched on
    purpose: String,
    user_message: String,
    /// Build plan Step 5: raw external content (e.g. a fetched Gmail
    /// thread) that must never be confused with a trusted instruction —
    /// see `model_gateway::build_prompt_with_untrusted_context`. `None`
    /// for ordinary owner-control turns (Step 4's only caller so far).
    #[serde(default)]
    untrusted_context: Option<String>,
    max_tokens: u32,
}

/// Which prompt template artifact an agent's `model.generate` calls
/// resolve to. A small, hardcoded map rather than a new
/// `AgentManifest.prompt_template` schema field: the PRD's `§10.1`/`§10.2`
/// agent fixtures are transcribed verbatim from the spec with no such
/// field, and the Phase 1-3 agent set is fixed and small (two agents) —
/// adding a schema field not in the PRD's literal example for a mapping
/// this simple isn't justified yet.
fn template_id_for_agent(agent_id: &str) -> Option<&'static str> {
    match agent_id {
        "main_assistant_agent" => Some("owner_control_template"),
        "email_reply_drafter" => Some("email_reply_draft_template"),
        _ => None,
    }
}

#[derive(Debug, Serialize)]
pub(super) struct GenerateResponseBody {
    decision: GateDecision,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
}

/// D-046: audits and returns a `limit_exceeded` denial before any payload
/// is stored or the provider is ever called — shared by both budget checks
/// in [`post_model_generate`], which differ only in which budget tripped.
fn deny_limit_exceeded(
    state: &AppState,
    action: &ActionId,
    grant_id: Ulid,
) -> Result<Json<GenerateResponseBody>, (StatusCode, Json<Value>)> {
    let decision = GateDecision::Deny {
        reason: DenialReason::LimitExceeded,
    };
    state
        .store
        .append_audit(
            "model.generate.gated",
            Some(action),
            Some(&decision),
            None,
            Some(grant_id),
            &[],
            &[],
        )
        .map_err(internal_error)?;
    Ok(Json(GenerateResponseBody {
        decision,
        text: None,
    }))
}

pub(super) async fn post_model_generate(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<GenerateRequestBody>,
) -> Result<Json<GenerateResponseBody>, (StatusCode, Json<Value>)> {
    let (grant, _pending_ref, _bound_chat_id) = authenticate(&state, &headers).await?;
    let now = Timestamp::now();
    let action = ActionId::new("model.generate:approved_provider");
    if !crate::spend::admit_spend(&state, crate::spend::SpendLane::from_grant(&grant), now)
        .await
        .map_err(internal_error)?
    {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"error": "daily_spend_cap_exceeded"})),
        ));
    }

    // D-046: grant budgets are enforced kernel-dispatch-side (same
    // placement precedent as selection-token single-use, see
    // `openspine_gate::gate`'s `GateContext` doc comment) — checked before
    // any payload is put or the new user turn is appended, so a limit of
    // `N` allows exactly `N` calls. Atomic upsert (not count-then-compare):
    // a plain `SELECT COUNT` read followed by a separate append left a
    // TOCTOU gap under concurrent requests on the same grant (found in
    // review) — two callers could both observe room under the limit before
    // either recorded a call. `try_count_model_call`'s `WHERE` clause makes
    // the check-and-increment one atomic statement, same as
    // `try_count_artifact_put` just below.
    if !state
        .store
        .try_count_model_call(grant.id, grant.limits.max_model_calls)
        .map_err(internal_error)?
    {
        return deny_limit_exceeded(&state, &action, grant.id);
    }
    if !state
        .store
        .try_count_artifact_put(grant.id, grant.limits.max_artifacts)
        .map_err(internal_error)?
    {
        return deny_limit_exceeded(&state, &action, grant.id);
    }

    let payload_value = json!({
        "purpose": body.purpose,
        "user_message": body.user_message,
        "untrusted_context": body.untrusted_context,
        "max_tokens": body.max_tokens,
    });
    let payload_ref = state
        .artifacts
        .put(canonical_json(&payload_value).as_bytes())
        .map_err(internal_error)?;

    let request = ActionRequest {
        id: Ulid::new(),
        task_grant_id: grant.id,
        action: action.clone(),
        target_ref: None,
        payload_ref: Some(payload_ref.clone()),
        target_digest: None,
        selection_token_id: None,
        params: std::collections::BTreeMap::new(),
        requested_at: now,
        schema_version: 1,
    };

    let outcome = gate(
        &grant,
        &request,
        ActionOrigin::Shell,
        &state.store,
        &state.action_catalog,
        &state.connectors,
        now,
    );
    state
        .store
        .append_audit(
            "model.generate.gated",
            Some(&action),
            Some(&outcome.decision),
            None,
            Some(grant.id),
            &[],
            std::slice::from_ref(&payload_ref),
        )
        .map_err(internal_error)?;

    let GateDecision::Allow = outcome.decision else {
        return Ok(Json(GenerateResponseBody {
            decision: outcome.decision,
            text: None,
        }));
    };

    let template_id = template_id_for_agent(&grant.agent_id)
        .ok_or_else(|| anyhow::anyhow!("agent {} has no known prompt template", grant.agent_id))
        .map_err(internal_error)?;
    // 5a: clone the template out of the shared-mutable registry under a
    // brief read guard; the subsequent provider call is `.await`.
    let template = state
        .registry
        .read()
        .templates
        .get(template_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("{template_id} not in registry"))
        .map_err(internal_error)?;

    // Persist the user turn, then load history (oldest-first, this turn
    // included) to build the prompt — conversation state stores only
    // role+digest (PRD §18: no raw content outside the artifact store), so
    // every turn's text is fetched back through `ArtifactStore::get`.
    let user_ref = state
        .artifacts
        .put(body.user_message.as_bytes())
        .map_err(internal_error)?;
    state
        .store
        .append_conversation_message(grant.id, "user", &user_ref.digest)
        .map_err(internal_error)?;

    let history = state
        .store
        .recent_conversation(grant.id, CONVERSATION_HISTORY_LIMIT)
        .map_err(internal_error)?;
    let mut conversation = Vec::with_capacity(history.len());
    for (role, digest) in history {
        let role = match role.as_str() {
            "user" => PromptRole::User,
            "assistant" => PromptRole::Assistant,
            other => {
                return Err(internal_error(format!("unknown conversation role {other}")));
            }
        };
        let bytes = state
            .artifacts
            .get(&ArtifactRef {
                digest,
                schema_version: 1,
            })
            .map_err(internal_error)?;
        conversation.push(PromptMessage {
            role,
            content: String::from_utf8_lossy(&bytes).into_owned(),
        });
    }
    let prompt = match &body.untrusted_context {
        Some(untrusted) => build_prompt_with_untrusted_context(
            &template,
            untrusted,
            conversation,
            body.max_tokens,
            ReasoningTier::Standard,
        ),
        None => build_prompt(
            &template,
            conversation,
            body.max_tokens,
            ReasoningTier::Standard,
        ),
    };
    let active_provider_id = state
        .active_model_providers
        .read()
        .get(&openspine_schemas::model_swap::ModelRole::Base)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no active base model provider"))
        .map_err(internal_error)?;
    let provider = state
        .gateway_tier_map
        .resolve(
            ReasoningTier::Standard,
            &active_provider_id,
            &state.provider_pool,
        )
        .ok_or_else(|| anyhow::anyhow!("active provider {active_provider_id} is unavailable"))
        .map_err(internal_error)?;
    let text = counted_model_generate(
        state.as_ref(),
        SpendLane::from_grant(&grant),
        provider,
        &prompt,
    )
    .await
    .map_err(|err| match err {
        SpendModelError::Provider(provider_err) => internal_error(provider_err),
        SpendModelError::Ledger(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "daily_spend_cap_unavailable"})),
        ),
        SpendModelError::Denied => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"error": "daily_spend_cap_exceeded"})),
        ),
    })?;

    let assistant_ref = state
        .artifacts
        .put(text.as_bytes())
        .map_err(internal_error)?;
    state
        .store
        .append_conversation_message(grant.id, "assistant", &assistant_ref.digest)
        .map_err(internal_error)?;

    Ok(Json(GenerateResponseBody {
        decision: GateDecision::Allow,
        text: Some(text),
    }))
}
