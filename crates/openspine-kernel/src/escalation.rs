//! Deterministic escalation routing and dormant thread↔grant binding
//! (AD-133, AD-148, AD-151).
//!
//! The pure surface function lives in `openspine_schemas::escalation`.
//! This module owns:
//! - the owner-delivery message format for escalations
//! - the dormant thread↔grant binding resolver

use openspine_schemas::escalation::{denial_reason_code, EscalationEvent, EscalationPayload};
use openspine_schemas::grant::TaskGrant;

/// Format the owner-facing escalation message for delivery on the owner
/// control channel. Deterministic from the typed event — no agent phrasing.
pub fn owner_escalation_message(event: &EscalationEvent) -> String {
    let (kind, summary) = match &event.payload {
        EscalationPayload::GateDenial { summary, .. } => ("gate_denial", summary),
        EscalationPayload::WorkerConfidence { summary } => ("worker_confidence", summary),
    };
    format!(
        "Escalation: task {} [{}] {}",
        event.task_grant_id, kind, summary
    )
}

/// Route one generic escalation to the task owner's bound control channel and
/// record the durable consequence. Future worker-runtime producers call this
/// same function with `WorkerConfidence`; the API handler is only one
/// producer. Destination is resolved from the persisted task grant, never
/// supplied by the producer.
pub(crate) async fn route_escalation(
    state: &crate::pipeline::AppState,
    grant: &TaskGrant,
    event: &EscalationEvent,
) -> Result<(), crate::store::StoreError> {
    debug_assert_eq!(grant.id, event.task_grant_id);
    let Some((stored_grant, _pending_ref, owner_chat_id)) =
        state.store.find_task_grant_by_id(event.task_grant_id)?
    else {
        return Err(crate::store::StoreError::TaskGrantNotFound(
            event.task_grant_id,
        ));
    };
    if stored_grant.id != grant.id {
        return Err(crate::store::StoreError::TaskGrantNotFound(
            event.task_grant_id,
        ));
    }
    let owner_message = owner_escalation_message(event);

    // AD-133: mandatory owner delivery uses the task's kernel-owned bound
    // owner chat. It returns an error on missing key, gate denial, or send
    // failure, so action.escalated is never recorded as a false success.
    crate::pipeline::notify_owner_required(state, owner_chat_id, &owner_message).await?;

    let (audit_kind, action, decision, reason) = match &event.payload {
        EscalationPayload::GateDenial {
            action,
            decision,
            reason,
            ..
        } => (
            "action.escalated",
            Some(action),
            Some(decision),
            Some(denial_reason_code(*reason)),
        ),
        EscalationPayload::WorkerConfidence { .. } => ("worker.escalated", None, None, None),
    };
    state
        .store
        .append_audit(
            audit_kind,
            action,
            decision,
            reason,
            Some(event.task_grant_id),
            &[],
            &[],
        )
        .map(|_| ())
}

/// AD-148: Kernel-owned thread↔grant binding. A reply in thread T resolves
/// to the grant bound to T; no binding → master thread (thread_id = None).
/// DORMANT: no production call site until a thread-capable channel ships.
#[allow(dead_code)] // AD-148: dormant until a thread-capable channel ships.
pub fn resolve_grant_for_thread<'a>(
    grants: &'a [TaskGrant],
    thread_id: Option<&str>,
) -> Option<&'a TaskGrant> {
    match thread_id {
        Some(tid) => grants
            .iter()
            .find(|g| g.thread_id.as_deref() == Some(tid))
            .or_else(|| grants.iter().find(|g| g.thread_id.is_none())),
        None => grants.iter().find(|g| g.thread_id.is_none()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::Timestamp;
    use openspine_schemas::action::{ActionId, DenialReason, GateDecision};
    use openspine_schemas::artifact::Lifecycle;
    use openspine_schemas::escalation::{surface_denial, EscalationEvent, CANONICAL_DEFERRAL};
    use openspine_schemas::grant::{GrantLimits, GrantMode};

    fn grant_with_thread(thread_id: Option<&str>) -> TaskGrant {
        let now = Timestamp::now();
        let id = ulid::Ulid::new();
        let mut g = TaskGrant {
            id,
            schema_version: 1,
            lifecycle_state: Lifecycle::Active,
            user: "owner".into(),
            purpose: "test".into(),
            issued_by: "kernel".into(),
            issued_at: now,
            expires_at: now + std::time::Duration::from_secs(60),
            event_id: ulid::Ulid::new(),
            route_id: "r".into(),
            agent_id: "a".into(),
            workflow_id: "w".into(),
            capability_pack_id: "p".into(),
            authority_sources: vec![],
            selection_tokens: vec![],
            allowed_actions: vec![],
            approval_required_actions: vec![],
            denied_actions: vec![],
            allowed_egress_classes: vec![],
            output_channels: vec![],
            limits: GrantLimits {
                max_model_calls: 1,
                max_artifacts: 1,
                max_runtime_seconds: 60,
            },
            task_token: "a".repeat(64),
            root_grant_id: id,
            parent_grant_id: None,
            mode: GrantMode::Live,
            chain: vec![],
            caveat_mac: String::new(),
            thread_id: thread_id.map(str::to_string),
        };
        g.seal_root(b"openspine-test-grant-hmac-key-v1");
        g
    }

    #[test]
    fn resolve_by_thread_id_returns_bound_grant() {
        let master = grant_with_thread(None);
        let bound = grant_with_thread(Some("t1"));
        let grants = [master, bound.clone()];
        let found = resolve_grant_for_thread(&grants, Some("t1")).unwrap();
        assert_eq!(found.id, bound.id);
    }

    #[test]
    fn no_thread_id_resolves_to_master() {
        let master = grant_with_thread(None);
        let bound = grant_with_thread(Some("t1"));
        let grants = [bound, master.clone()];
        let found = resolve_grant_for_thread(&grants, None).unwrap();
        assert_eq!(found.id, master.id);
        assert!(found.thread_id.is_none());
    }

    #[test]
    fn unknown_thread_id_falls_back_to_master() {
        let master = grant_with_thread(None);
        let bound = grant_with_thread(Some("t1"));
        let grants = [bound, master.clone()];
        let found = resolve_grant_for_thread(&grants, Some("missing")).unwrap();
        assert_eq!(found.id, master.id);
        assert!(found.thread_id.is_none());
    }

    #[test]
    fn owner_message_carries_action_and_reason_code() {
        let grant = grant_with_thread(None);
        let action = ActionId::new("email.send");
        let decision = GateDecision::Deny {
            reason: DenialReason::ExplicitDeny,
        };
        let (_, notice) =
            surface_denial(&grant, &action, &decision, None, Timestamp::now()).unwrap();
        let event = EscalationEvent::from_denial(&notice);
        let msg = owner_escalation_message(&event);
        assert!(msg.contains("email.send"));
        assert!(msg.contains("explicit_deny"));
        assert!(msg.contains(&grant.id.to_string()));
    }

    #[test]
    fn thread_id_defaults_to_none_on_fresh_grant() {
        let g = grant_with_thread(None);
        assert!(g.thread_id.is_none());
    }

    #[test]
    fn counterparty_deferral_text_is_canonical() {
        let grant = grant_with_thread(None);
        let action = ActionId::new("email.send");
        let (deferral, _) = surface_denial(
            &grant,
            &action,
            &GateDecision::Deny {
                reason: DenialReason::NotGranted,
            },
            None,
            Timestamp::now(),
        )
        .unwrap();
        assert_eq!(deferral.text, CANONICAL_DEFERRAL);
    }
}
