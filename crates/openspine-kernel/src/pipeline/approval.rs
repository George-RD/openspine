//! Draft-approval callback handling (build plan Step 6, D-039/D-044): the
//! owner's tap on the inline "Approve" button attached to `lyra.ui.preview`
//! (D-043) is the entire trust boundary for "did the owner approve this
//! exact draft" — recording the digest-bound approval, re-running
//! `gate()`, and — only on `Allow` — actually creating the Gmail draft all
//! happen kernel-side here, without spawning a new shell (D-044: the shell
//! that requested the preview is long gone by the time a human taps a
//! button).

use jiff::Timestamp;
use openspine_gate::gate;
use openspine_schemas::action::{ActionRequest, GateDecision};
use openspine_schemas::approval::{ApprovalDecision, ApprovalRecord, TimeoutBehavior};
use openspine_schemas::digest::digest_of;
use openspine_schemas::grant::TaskGrant;
use serde_json::json;
use ulid::Ulid;

use super::{notify_owner_best_effort, AppState};

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
    state
        .telegram
        .answer_callback_query(callback_query_id)
        .await;

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
    if !state.store.try_consume_action_request(action_request_id)? {
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

    let outcome = gate(&grant, &request, &state.store, now);
    match outcome.decision {
        GateDecision::Allow => create_approved_draft(state, &grant, &request, chat_id).await,
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

/// Actually create the Gmail draft, only ever reached after `gate()` has
/// confirmed a matching, unexpired approval. Re-derives the reply target
/// fresh from a live Gmail fetch (D-042) and — critically — re-checks it
/// against the digest bound at proposal time (D-041) before ever calling
/// [`crate::gmail::GmailConnector::create_draft`]: the thread may have
/// received a new message between proposal and approval, which would
/// silently change who "the newest non-owner sender" is. Content
/// addressing already guarantees the payload (`subject`/`body`) can't have
/// changed; the target is not content-addressed, so this is the one place
/// a "approved draft A, but thread now points at draft B" mismatch could
/// slip through undetected without an explicit re-check (spec.md's
/// "Recipient changes after approval" scenario).
async fn create_approved_draft(
    state: &AppState,
    grant: &TaskGrant,
    request: &ActionRequest,
    chat_id: i64,
) -> anyhow::Result<()> {
    let payload_ref = request
        .payload_ref
        .as_ref()
        .expect("checked by handle_draft_approval_callback before dispatch");
    let bytes = state.artifacts.get(payload_ref)?;
    let payload: serde_json::Value = serde_json::from_slice(&bytes)?;
    let subject = payload["subject"].as_str().unwrap_or_default();
    let body = payload["body"].as_str().unwrap_or_default();
    let thread_id = request
        .target_ref
        .as_ref()
        .and_then(|t| t.id.clone())
        .unwrap_or_default();

    let Some(gmail) = &state.gmail else {
        state.store.append_audit(
            "draft.creation_failed",
            Some(&request.action),
            None,
            Some("no gmail connector configured"),
            Some(grant.id),
            &[],
            &[],
        )?;
        notify_owner_best_effort(
            state,
            chat_id,
            "Approved, but Gmail isn't configured anymore — couldn't create the draft.",
        )
        .await;
        return Ok(());
    };

    let thread = match gmail.fetch_thread(&thread_id).await {
        Ok(thread) => thread,
        Err(err) => {
            state.store.append_audit(
                "draft.creation_failed",
                Some(&request.action),
                None,
                Some(&err.to_string()),
                Some(grant.id),
                &[],
                &[],
            )?;
            notify_owner_best_effort(
                state,
                chat_id,
                "Approved, but couldn't reach Gmail to create the draft — try /draft again.",
            )
            .await;
            return Ok(());
        }
    };
    let Some(target) = crate::gmail::newest_non_owner_recipient(&thread, gmail.mailbox_address())
    else {
        state.store.append_audit(
            "draft.creation_failed",
            Some(&request.action),
            None,
            Some("no non-owner recipient found in thread"),
            Some(grant.id),
            &[],
            &[],
        )?;
        notify_owner_best_effort(
            state,
            chat_id,
            "Approved, but couldn't determine who to reply to.",
        )
        .await;
        return Ok(());
    };

    // D-041/D-011: the target must still match exactly what was
    // digest-bound at proposal time — same shape `propose_draft_creation`
    // hashed. A mismatch means the thread changed since approval; the
    // draft must not be created with a target the owner never reviewed.
    let current_target_digest = digest_of(&json!({
        "thread_id": thread_id,
        "connector": "gmail_primary",
        "account_role": "owner_mailbox",
        "recipients": [target.recipient],
    }));
    if Some(&current_target_digest) != request.target_digest.as_ref() {
        state.store.append_audit(
            "draft.target_mutated_since_approval",
            Some(&request.action),
            None,
            Some("recomputed target digest no longer matches the approved one"),
            Some(grant.id),
            &[],
            &[],
        )?;
        notify_owner_best_effort(
            state,
            chat_id,
            "The thread changed since you approved this draft — please run /draft again.",
        )
        .await;
        return Ok(());
    }

    match gmail.create_draft(&thread_id, &target, subject, body).await {
        Ok(draft_id) => {
            // Best-effort: a completed Gmail draft must still be audited
            // and the owner still told it succeeded even if persisting
            // the draft-id ref fails — losing that ref is far cheaper
            // than losing the audit record of a real provider-side
            // mutation that already happened.
            let draft_id_refs = match state.artifacts.put(draft_id.as_bytes()) {
                Ok(r) => vec![r],
                Err(err) => {
                    tracing::warn!(error = %err, "failed to store draft_id artifact ref");
                    vec![]
                }
            };
            state.store.append_audit(
                "draft.created",
                Some(&request.action),
                None,
                None,
                Some(grant.id),
                &[],
                &draft_id_refs,
            )?;
            notify_owner_best_effort(state, chat_id, "Draft created in Gmail.").await;
        }
        Err(err) => {
            state.store.append_audit(
                "draft.creation_failed",
                Some(&request.action),
                None,
                Some(&err.to_string()),
                Some(grant.id),
                &[],
                &[],
            )?;
            notify_owner_best_effort(
                state,
                chat_id,
                "Approved, but Gmail rejected the draft — try /draft again.",
            )
            .await;
        }
    }
    Ok(())
}
