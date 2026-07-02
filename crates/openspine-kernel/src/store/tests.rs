use super::*;
use openspine_schemas::action::DenialReason;
use openspine_schemas::approval::{ApprovalDecision, TimeoutBehavior};
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::grant::GrantLimits;
use openspine_schemas::selection::{
    SelectionScope, SelectionTokenType, SelectionVerificationMethod,
};

fn sample_grant(task_token: &str) -> TaskGrant {
    let issued_at = Timestamp::now();
    TaskGrant {
        id: Ulid::new(),
        schema_version: 1,
        lifecycle_state: Lifecycle::Active,
        user: "owner".to_string(),
        purpose: "test".to_string(),
        issued_by: "kernel".to_string(),
        issued_at,
        expires_at: issued_at + std::time::Duration::from_secs(120),
        event_id: Ulid::new(),
        route_id: "owner_telegram_main_assistant".to_string(),
        agent_id: "main_assistant_agent".to_string(),
        workflow_id: "owner_control_conversation".to_string(),
        capability_pack_id: "owner_control_basic_pack".to_string(),
        authority_sources: vec![],
        selection_tokens: vec![],
        allowed_actions: vec![ActionId::new("openspine.status.read")],
        approval_required_actions: vec![],
        denied_actions: vec![],
        output_channels: vec![],
        limits: GrantLimits {
            max_model_calls: 8,
            max_artifacts: 20,
            max_runtime_seconds: 120,
        },
        task_token: task_token.to_string(),
    }
}

fn sample_approval(action_request_id: Ulid) -> ApprovalRecord {
    let now = Timestamp::now();
    ApprovalRecord {
        id: Ulid::new(),
        schema_version: 1,
        action_request_id,
        approved_by: "owner".to_string(),
        approved_at: now,
        approved_payload_digest: Digest::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
        approved_target_digest: Digest::parse(format!("sha256:{}", "b".repeat(64))).unwrap(),
        expires_at: now + std::time::Duration::from_secs(900),
        decision: ApprovalDecision::Approved,
        timeout_behavior: TimeoutBehavior::DoNothing,
        approval_channel: "telegram_inline".to_string(),
    }
}

fn sample_selection_token() -> SelectionToken {
    let now = Timestamp::now();
    SelectionToken {
        id: Ulid::new(),
        schema_version: 1,
        token_type: SelectionTokenType::EmailThreadSelection,
        user: "owner".to_string(),
        target_id: "thread-1".to_string(),
        selected_by: "owner".to_string(),
        selected_at: now,
        issued_by: "kernel".to_string(),
        expires_at: now + std::time::Duration::from_secs(600),
        verified_source: true,
        verification_method: SelectionVerificationMethod::KernelUiSelection,
        connector: None,
        account_role: None,
        scope: SelectionScope {
            read_thread: true,
            attachments_allowed: false,
            max_messages: 20,
            include_headers: true,
            include_recipients: true,
            include_body: true,
        },
        single_use: true,
    }
}

#[test]
fn task_grant_round_trips_by_token() {
    let store = Store::open_in_memory().unwrap();
    let grant = sample_grant("token-a");
    let pending_message_ref = ArtifactRef {
        digest: Digest::parse(format!("sha256:{}", "c".repeat(64))).unwrap(),
        schema_version: 1,
    };
    store
        .insert_task_grant(&grant, &pending_message_ref, 555)
        .unwrap();
    let (back, back_ref, bound_chat_id) =
        store.find_task_grant_by_token("token-a").unwrap().unwrap();
    assert_eq!(back, grant);
    assert_eq!(back_ref, pending_message_ref);
    assert_eq!(bound_chat_id, 555);
    assert!(store
        .find_task_grant_by_token("no-such-token")
        .unwrap()
        .is_none());
}

#[test]
fn first_audit_row_chains_from_genesis() {
    let store = Store::open_in_memory().unwrap();
    let event = store
        .append_audit("kernel.started", None, None, None, None, &[], &[])
        .unwrap();
    assert_eq!(event.prev_hash, genesis_digest());
    assert_ne!(event.hash, genesis_digest());
}

#[test]
fn second_audit_row_chains_from_first_hash() {
    let store = Store::open_in_memory().unwrap();
    let first = store
        .append_audit("kernel.started", None, None, None, None, &[], &[])
        .unwrap();
    let second = store
        .append_audit("kernel.started", None, None, None, None, &[], &[])
        .unwrap();
    assert_eq!(second.prev_hash, first.hash);
    assert_ne!(second.hash, first.hash);
}

#[test]
fn empty_chain_verifies_true() {
    let store = Store::open_in_memory().unwrap();
    assert!(store.verify_audit_chain().unwrap());
}

#[test]
fn intact_chain_verifies_true() {
    let store = Store::open_in_memory().unwrap();
    for _ in 0..5 {
        store
            .append_audit(
                "action.gate_decision",
                Some(&ActionId::new("openspine.status.read")),
                Some(&GateDecision::Deny {
                    reason: DenialReason::NotGranted,
                }),
                Some("not_granted"),
                None,
                &[],
                &[],
            )
            .unwrap();
    }
    assert!(store.verify_audit_chain().unwrap());
}

#[test]
fn tampered_meta_json_breaks_verification() {
    let store = Store::open_in_memory().unwrap();
    store
        .append_audit("kernel.started", None, None, None, None, &[], &[])
        .unwrap();
    {
        let conn = store.conn.lock();
        conn.execute(
            "UPDATE audit_log SET meta_json = '{\"tampered\":true}' WHERE seq = 1",
            [],
        )
        .unwrap();
    }
    assert!(!store.verify_audit_chain().unwrap());
}

#[test]
fn tampered_hash_breaks_verification() {
    let store = Store::open_in_memory().unwrap();
    store
        .append_audit("kernel.started", None, None, None, None, &[], &[])
        .unwrap();
    {
        let conn = store.conn.lock();
        conn.execute(
            &format!(
                "UPDATE audit_log SET hash = 'sha256:{}' WHERE seq = 1",
                "f".repeat(64)
            ),
            [],
        )
        .unwrap();
    }
    assert!(!store.verify_audit_chain().unwrap());
}

#[test]
fn approval_round_trips_by_action_request_id() {
    let store = Store::open_in_memory().unwrap();
    let action_request_id = Ulid::new();
    let approval = sample_approval(action_request_id);
    store.insert_approval(&approval).unwrap();
    let back = store
        .find_approval_for_request(action_request_id)
        .unwrap()
        .unwrap();
    assert_eq!(back, approval);
    assert!(store
        .find_approval_for_request(Ulid::new())
        .unwrap()
        .is_none());
}

#[test]
fn most_recent_approval_wins_when_multiple_exist_for_one_request() {
    let store = Store::open_in_memory().unwrap();
    let action_request_id = Ulid::new();
    let mut first = sample_approval(action_request_id);
    first.decision = ApprovalDecision::Rejected;
    store.insert_approval(&first).unwrap();
    let second = sample_approval(action_request_id);
    store.insert_approval(&second).unwrap();

    let back = store
        .find_approval_for_request(action_request_id)
        .unwrap()
        .unwrap();
    assert_eq!(back, second);
}

#[test]
fn selection_token_round_trips_and_marks_used() {
    let store = Store::open_in_memory().unwrap();
    let token = sample_selection_token();
    store.insert_selection_token(&token).unwrap();
    let back = store.find_selection_token(token.id).unwrap().unwrap();
    assert_eq!(back, token);

    store.mark_selection_token_used(token.id).unwrap();
    let conn = store.conn.lock();
    let used: i64 = conn
        .query_row(
            "SELECT used FROM selection_tokens WHERE id = ?1",
            params![token.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(used, 1);
}

#[test]
fn conversation_history_returns_oldest_first_within_limit() {
    let store = Store::open_in_memory().unwrap();
    let task_grant_id = Ulid::new();
    for i in 0..5 {
        let digest = Digest::parse(format!("sha256:{}", i.to_string().repeat(64))).unwrap();
        store
            .append_conversation_message(task_grant_id, "user", &digest)
            .unwrap();
    }
    let recent = store.recent_conversation(task_grant_id, 3).unwrap();
    assert_eq!(recent.len(), 3);
    // Oldest-first within the returned (most recent 3) window: message 2, 3, 4.
    let expected_2 = Digest::parse(format!("sha256:{}", "2".repeat(64))).unwrap();
    let expected_4 = Digest::parse(format!("sha256:{}", "4".repeat(64))).unwrap();
    assert_eq!(recent[0].1, expected_2);
    assert_eq!(recent[2].1, expected_4);
}

#[test]
fn kv_state_round_trips_and_upserts() {
    let store = Store::open_in_memory().unwrap();
    assert!(store.get_kv("last_telegram_update_id").unwrap().is_none());
    store.set_kv("last_telegram_update_id", "42").unwrap();
    assert_eq!(
        store.get_kv("last_telegram_update_id").unwrap(),
        Some("42".to_string())
    );
    store.set_kv("last_telegram_update_id", "43").unwrap();
    assert_eq!(
        store.get_kv("last_telegram_update_id").unwrap(),
        Some("43".to_string())
    );
}
