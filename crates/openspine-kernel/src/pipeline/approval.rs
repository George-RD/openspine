//! Digest-bound approval callback handling (build plan Step 6, D-039/D-044,
//! extended by 5d for `artifact.activate`): the owner's tap on the inline
//! "Approve" button attached to `lyra.ui.preview` or `artifact.propose` is
//! the entire trust boundary for "did the owner approve this exact draft /
//! artifact" — recording the digest-bound approval, re-running `gate()`,
//! and — only on `Allow` — actually creating the Gmail draft or activating
//! the artifact all happen kernel-side here, without spawning a new shell
//! (D-044: the shell that requested the preview is long gone by the time a
//! human taps a button).

use jiff::Timestamp;
use openspine_gate::{gate, ActionOrigin};
use openspine_schemas::action::GateDecision;
use openspine_schemas::approval::{ApprovalDecision, ApprovalRecord, TimeoutBehavior};
use ulid::Ulid;

use super::post_approval::resolve_post_approval_handler;
use super::{notify_owner_best_effort, AppState};
#[path = "approval_draft.rs"]
mod approval_draft;
pub(super) use approval_draft::create_approved_draft;

/// How long a freshly minted approval remains valid (PRD §15-style
/// reasoning applied to D-011 approvals): this handler mints the approval
/// and immediately re-runs `gate()` against it in the same call, so the
/// TTL is not load-bearing for normal operation — it exists so an approval
/// record is never technically "valid forever" if something ever reads it
/// back later.
const APPROVAL_TTL: std::time::Duration = std::time::Duration::from_secs(300);

/// Handle a tap on the "Approve" button. Same `Ok(())`-always-audited
/// contract as the rest of this pipeline: every outcome the pipeline
/// itself decides on (unknown request, foreign chat, gate denial, Gmail
/// failure) is logged and swallowed into `Ok(())` — only a genuine
/// infrastructure failure (store I/O) surfaces as `Err`.
pub(super) async fn handle_draft_approval_callback(
    state: &AppState,
    chat_id: i64,
    callback_query_id: &str,
    action_request_id: Ulid,
) -> anyhow::Result<()> {
    crate::spend::guard_connector(state, true).await?;
    let answer_result = crate::api::connector_breaker::call_with_connector_preflight(
        state,
        "telegram",
        None,
        state
            .connectors
            .telegram()
            .answer_callback_query(callback_query_id),
    )
    .await;
    // A callback ack is pure control-plane bookkeeping (it only stops the
    // tapper's spinner). It must never abort the approval it accompanies, so
    // its telemetry is recorded best-effort and the approval proceeds
    // regardless (PI parent note / FsReviewSec P1: a failed ack must not
    // prevent an approval from completing, and must not be swallowed
    // silently either).
    crate::failure_surfacing::record_callback_ack(
        state,
        answer_result.is_ok(),
        answer_result
            .as_ref()
            .err()
            .map(|e| e.to_string())
            .as_deref(),
    );

    let Some(request) = state.store.find_action_request(action_request_id)? else {
        state.store.append_audit(
            "draft.approval_unknown_request",
            None,
            None,
            Some("action_request_id not found"),
            None,
            &[],
            &[],
        )?;
        notify_owner_best_effort(state, chat_id, "That approval request is no longer valid.").await;
        return Ok(());
    };
    // artifact.reconfirm requests are minted at startup with a reserved
    // task_grant_id but no grant row; mint a fresh short-lived owner-bound
    // grant at tap time so the normal channel-bind/consume/gate/post-approval
    // path runs unchanged. Authority TTL starts only here, not at mint.
    if request.action.as_str() == "artifact.reconfirm" {
        let existing = state.store.find_task_grant_by_id(request.task_grant_id)?;
        let refresh = existing
            .as_ref()
            .is_none_or(|(grant, _, _)| grant.is_expired(Timestamp::now()));
        if refresh {
            if let Some(grant) = super::mint_reconfirm_grant(request.task_grant_id) {
                if existing.is_some() {
                    state.store.refresh_task_grant(&grant)?;
                } else {
                    let pending_ref = state.artifacts.put(b"reconfirm-synthetic-pending")?.clone();
                    state
                        .store
                        .insert_task_grant(&grant, &pending_ref, chat_id)?;
                }
            }
        }
    }

    let Some((grant, _pending_ref, bound_chat_id)) =
        state.store.find_task_grant_by_id(request.task_grant_id)?
    else {
        state.store.append_audit(
            "draft.approval_grant_missing",
            Some(&request.action),
            None,
            Some("task grant behind this request no longer exists"),
            Some(request.task_grant_id),
            &[],
            &[],
        )?;
        notify_owner_best_effort(state, chat_id, "The task behind that draft is gone.").await;
        return Ok(());
    };

    // Channel binding by construction (same principle as
    // `telegram.reply:owner_channel`'s bound-chat check): only the grant's
    // own bound chat may approve the request it proposed.
    if bound_chat_id != chat_id {
        state.store.append_audit(
            "draft.approval_channel_mismatch",
            Some(&request.action),
            None,
            Some("callback chat_id does not match the grant's bound chat"),
            Some(grant.id),
            &[],
            &[],
        )?;
        return Ok(());
    }

    // D-044: the "Approve" button stays live on the Telegram message
    // forever (Telegram doesn't remove inline keyboards, and
    // `answerCallbackQuery` doesn't disable them) — atomically consume the
    // request before minting an approval or touching Gmail, so a second
    // tap, or Telegram redelivering the same `callback_query` update,
    // audits as "already handled" instead of minting a second
    // `ApprovalRecord` and creating a second Gmail draft. Deliberately
    // placed after the grant-exists and channel-binding checks above: a
    // callback from the wrong chat (or for a dead grant) must stay a
    // no-op deny, not burn the request and permanently kill the real
    // owner's Approve button.
    // Reconfirm requests consume the action request *inside* the durable
    // reconfirm transaction (so a failed commit leaves it retryable); every
    // other request is consumed here, before any side effect, to prevent a
    // second tap from minting a duplicate approval/draft.
    if request.action.as_str() != "artifact.reconfirm"
        && !state.store.try_consume_action_request(action_request_id)?
    {
        state.store.append_audit(
            "draft.approval_already_handled",
            Some(&request.action),
            None,
            Some("action request was already approved or is being approved"),
            Some(grant.id),
            &[],
            &[],
        )?;
        return Ok(());
    }

    let (Some(payload_ref), Some(target_digest)) = (&request.payload_ref, &request.target_digest)
    else {
        // Structurally unreachable given `propose_draft_creation` always
        // sets both — `gate()` would deny anyway on a missing digest, but
        // this is handled explicitly rather than relying on that.
        state.store.append_audit(
            "draft.approval_request_malformed",
            Some(&request.action),
            None,
            Some("proposed request is missing a payload or target digest"),
            Some(grant.id),
            &[],
            &[],
        )?;
        notify_owner_best_effort(
            state,
            chat_id,
            "That draft proposal is malformed and cannot be approved.",
        )
        .await;
        return Ok(());
    };

    let now = Timestamp::now();
    let approval = ApprovalRecord {
        id: Ulid::new(),
        schema_version: 1,
        action_request_id: request.id,
        approved_by: state.owner_user_id.to_string(),
        approved_at: now,
        approved_payload_digest: payload_ref.digest.clone(),
        approved_target_digest: target_digest.clone(),
        expires_at: now + APPROVAL_TTL,
        decision: ApprovalDecision::Approved,
        timeout_behavior: TimeoutBehavior::DoNothing,
        approval_channel: "telegram_inline".to_string(),
    };
    state.store.insert_approval(&approval)?;
    state.store.append_audit(
        "approval.recorded",
        Some(&request.action),
        None,
        None,
        Some(grant.id),
        &[],
        &[],
    )?;

    let outcome = gate(
        &grant,
        &request,
        ActionOrigin::Shell,
        &state.store,
        &state.action_catalog,
        &state.connectors,
        now,
    );
    state.store.append_audit(
        "action.gated",
        Some(&request.action),
        Some(&outcome.decision),
        None,
        Some(grant.id),
        &[],
        std::slice::from_ref(payload_ref),
    )?;
    match outcome.decision {
        GateDecision::Allow => {
            let handler = resolve_post_approval_handler(&request.action);
            handler(state, &grant, &request, chat_id).await
        }
        other => {
            state.store.append_audit(
                "draft.approval_gate_denied",
                Some(&request.action),
                Some(&other),
                None,
                Some(grant.id),
                &[],
                &[],
            )?;
            notify_owner_best_effort(
                state,
                chat_id,
                "Couldn't approve that draft — please run /draft again.",
            )
            .await;
            Ok(())
        }
    }
}
