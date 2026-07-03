//! The `/draft <thread_id>` selected-thread selection flow (build plan
//! Step 5, D-036/D-037): verify the Gmail thread exists, mint a single-use
//! [`SelectionToken`], and compose authority for `email_reply_drafter` as a
//! new task — entirely separate from [`super::handle_owner_update`]'s
//! normal `owner_telegram_main_assistant` routing.

use jiff::Timestamp;
use openspine_authority::{compose_authority, resolve_route, AuthorityInput, AuthorityOutcome};
use openspine_schemas::event::{
    AccountRole, ActorHint, ChannelTrust, Connector, DataClassification, EventEnvelope, EventType,
    InteractionMode, Lane, Source, TargetRef, TargetRefKind, TrustContext, VerificationMethod,
};
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::identity::RelationshipKind;
use openspine_schemas::route::RouteResolution;
use openspine_schemas::selection::{
    SelectionScope, SelectionToken, SelectionTokenType, SelectionVerificationMethod,
};

use crate::sandbox;
use ulid::Ulid;

use super::{empty_session_policy, notify_owner_best_effort, resolve_owner_identity, AppState};

/// Bound how long a freshly minted selection token remains valid (PRD §15:
/// "expires quickly") — generous enough to survive the Gmail existence
/// check and the resulting authority composition without racing its own
/// expiry, tight enough that a forgotten `/draft` never lingers.
const SELECTION_TOKEN_TTL: std::time::Duration = std::time::Duration::from_secs(300);

/// Handle a recognized `/draft <thread_id>` owner command (D-036): verify
/// the thread exists via Gmail, mint a single-use [`SelectionToken`], build
/// the `email.thread.selected` event, and compose authority for
/// `email_reply_drafter` as a new task — entirely separate from
/// [`super::handle_owner_update`]'s normal `owner_telegram_main_assistant`
/// routing (PRD §21.1's Phase-2 selected-thread-email workflow, steps 1-9).
///
/// Same `Ok(None)` vs `Ok(Some(grant))` vs `Err` contract as
/// [`super::handle_owner_update`]: every pipeline-decided outcome (no Gmail
/// configured, thread not found, route/authority denial) is `Ok(None)`
/// with an audit row; only a genuine infrastructure failure is `Err`.
pub(super) async fn handle_thread_selection(
    state: &AppState,
    chat_id: i64,
    thread_id: &str,
) -> anyhow::Result<Option<TaskGrant>> {
    let Some(gmail) = &state.gmail else {
        state.store.append_audit(
            "selection.gmail_not_configured",
            None,
            None,
            Some("no gmail connector configured; /draft is unavailable"),
            None,
            &[],
            &[],
        )?;
        notify_owner_best_effort(
            state,
            chat_id,
            "Gmail isn't configured on this kernel yet, so /draft is unavailable.",
        )
        .await;
        return Ok(None);
    };

    // D-025 / O-003 / PRD §16: refuse before ever contacting Gmail or
    // minting a token — this workflow's lane is statically
    // `Lane::ExternalCommunication`, so the guard needs no envelope to
    // evaluate. Checking this only after `thread_exists`/token-mint (as a
    // prior revision did) meant a refused request still burned a live
    // Gmail API call and left an orphaned, never-granted selection token
    // sitting in the store.
    if sandbox::refuses_external_communication_without_containment(
        Lane::ExternalCommunication,
        &state.sandbox,
        state.unsafe_allow_uncontained_private_data,
    ) {
        state.store.append_audit(
            "route.refused_uncontained",
            None,
            None,
            Some("external_communication lane requires a containing sandbox driver"),
            None,
            &[],
            &[],
        )?;
        return Ok(None);
    }

    // The kernel itself proves the thread is real before ever minting a
    // token for it — a selection token must never be issued for something
    // Gmail can't actually serve.
    match gmail.thread_exists(thread_id).await {
        Ok(true) => {}
        Ok(false) => {
            state.store.append_audit(
                "selection.thread_not_found",
                None,
                None,
                Some(thread_id),
                None,
                &[],
                &[],
            )?;
            notify_owner_best_effort(
                state,
                chat_id,
                &format!("Couldn't find a Gmail thread with id \"{thread_id}\"."),
            )
            .await;
            return Ok(None);
        }
        Err(err) => {
            state.store.append_audit(
                "selection.gmail_error",
                None,
                None,
                Some(&err.to_string()),
                None,
                &[],
                &[],
            )?;
            notify_owner_best_effort(
                state,
                chat_id,
                "Couldn't reach Gmail just now — try again shortly.",
            )
            .await;
            return Ok(None);
        }
    }

    let now = Timestamp::now();
    let user = state.owner_user_id.to_string();
    let token = SelectionToken {
        id: Ulid::new(),
        schema_version: 1,
        token_type: SelectionTokenType::EmailThreadSelection,
        user: user.clone(),
        target_id: thread_id.to_string(),
        selected_by: user.clone(),
        selected_at: now,
        issued_by: "kernel".to_string(),
        expires_at: now + SELECTION_TOKEN_TTL,
        verified_source: true,
        // D-036: the trigger arrived over the already-verified
        // owner-control Telegram channel, distinct from a kernel-owned
        // picker UI (`KernelUiSelection`, used on the *event* below).
        verification_method: SelectionVerificationMethod::ApprovedOwnerControlSelection,
        connector: Some(Connector::GmailPrimaryConnector),
        account_role: Some(AccountRole::OwnerMailbox),
        scope: SelectionScope {
            read_thread: true,
            attachments_allowed: false,
            max_messages: 20,
            include_headers: true,
            include_recipients: true,
            include_body: true,
        },
        single_use: true,
    };

    let raw_ref = state.artifacts.put(thread_id.as_bytes())?;
    let envelope = EventEnvelope {
        id: Ulid::new(),
        source: Source::Gmail,
        connector: Some(Connector::GmailPrimaryConnector),
        account_role: Some(AccountRole::OwnerMailbox),
        event_type: EventType::EmailThreadSelected,
        received_at: now,
        verified_source: true,
        verification_method: VerificationMethod::KernelUiSelection,
        replay_protected: false,
        replay_nonce: None,
        channel_account: user.clone(),
        raw_event_ref: raw_ref.clone(),
        actor_hint: ActorHint::default(),
        target_refs: vec![TargetRef {
            kind: TargetRefKind::EmailThread,
            id: Some(thread_id.to_string()),
        }],
        data_classification: DataClassification::Private,
        user_intent_hint: Some("draft_reply_for_selected_email_thread".to_string()),
        lane: Lane::ExternalCommunication,
        trust_context: TrustContext {
            channel_trust: ChannelTrust::OwnerDevice,
            interaction_mode: InteractionMode::UserSelected,
        },
        schema_version: 1,
    };
    state.store.append_audit(
        "event.received",
        None,
        None,
        None,
        None,
        &[],
        std::slice::from_ref(&raw_ref),
    )?;

    let identity = resolve_owner_identity(&envelope, ChannelTrust::OwnerDevice);
    // 5a: clone the route table out of the shared-mutable registry under a
    // brief read guard; never held across the `.await` calls below.
    let routes = state.registry.read().routes.clone();
    let route_resolution =
        resolve_route(&envelope, &identity, Some(RelationshipKind::Owner), &routes);
    let route_id = match route_resolution {
        RouteResolution::Success { route_id } => route_id,
        RouteResolution::Denied { reason } => {
            state
                .store
                .append_audit("route.denied", None, None, Some(&reason), None, &[], &[])?;
            return Ok(None);
        }
        RouteResolution::Ambiguous { reason, .. } => {
            state.store.append_audit(
                "route.ambiguous",
                None,
                None,
                Some(&reason),
                None,
                &[],
                &[],
            )?;
            return Ok(None);
        }
    };

    let route = routes
        .iter()
        .find(|r| r.id == route_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("resolved route {route_id} not found in registry"))?;

    let agent_id = route
        .agent
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("route {route_id} names no agent"))?;
    let workflow_id = route
        .workflow
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("route {route_id} names no workflow"))?;
    let pack_id = route
        .capability_pack
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("route {route_id} names no capability_pack"))?;

    let (agent, workflow, pack, global_policy) = {
        let registry = state.registry.read();
        let agent = registry
            .agents
            .get(agent_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("agent {agent_id} not in registry"))?;
        let workflow = registry
            .workflows
            .get(workflow_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("workflow {workflow_id} not in registry"))?;
        let pack = registry
            .packs
            .get(pack_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("capability_pack {pack_id} not in registry"))?;
        let global_policy = registry
            .policies
            .get("global")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("global policy not in registry"))?;
        (agent, workflow, pack, global_policy)
    };
    let session = empty_session_policy();

    let input = AuthorityInput {
        event: &envelope,
        identity: &identity,
        route: &route,
        global_policy: &global_policy,
        agent: &agent,
        workflow: &workflow,
        pack: &pack,
        session: &session,
        user: &user,
        purpose: "selected_thread_email_reply_draft",
    };

    let mut grant = match compose_authority(&input, now) {
        AuthorityOutcome::Granted(grant) => *grant,
        AuthorityOutcome::Denied { reason } => {
            state.store.append_audit(
                "authority.denied",
                None,
                None,
                Some(&reason),
                None,
                &[],
                &[],
            )?;
            return Ok(None);
        }
        AuthorityOutcome::Ambiguous { .. } => {
            state.store.append_audit(
                "authority.ambiguous",
                None,
                None,
                Some("compose_authority returned Ambiguous, which it is not expected to produce"),
                None,
                &[],
                &[],
            )?;
            return Ok(None);
        }
    };

    // PRD §15: the token is only ever persisted once every earlier
    // refusal path (containment, route, authority) has already returned —
    // an orphaned, never-granted token must never reach the store.
    state.store.insert_selection_token(&token)?;

    // PRD §12's `selection_tokens`: this grant may use exactly the one
    // token minted for it. `compose_authority` has no selection-flow
    // concept and never populates this field — the caller that actually
    // minted the token is responsible for binding it here.
    grant.selection_tokens = vec![token.id];

    let pending_ref = state.artifacts.put(
        format!(
            "Draft a reply to Gmail thread {thread_id} (selection token {})",
            token.id
        )
        .as_bytes(),
    )?;
    state
        .store
        .insert_task_grant(&grant, &pending_ref, chat_id)?;
    state.store.append_audit(
        "authority.granted",
        None,
        None,
        None,
        Some(grant.id),
        &[],
        &[pending_ref],
    )?;

    match state
        .sandbox
        .run_task(&state.kernel_endpoint, &grant.task_token)
        .await
    {
        Ok(()) => {
            state.store.append_audit(
                "task.shell_completed",
                None,
                None,
                None,
                Some(grant.id),
                &[],
                &[],
            )?;
        }
        Err(err) => {
            state.store.append_audit(
                "task.shell_failed",
                None,
                None,
                Some(&err.to_string()),
                Some(grant.id),
                &[],
                &[],
            )?;
        }
    }

    Ok(Some(grant))
}
