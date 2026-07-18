use super::*;
use openspine_schemas::action::{ActionId, GateDecision};
use openspine_schemas::action::{ActionRequest, DenialReason};
use openspine_schemas::approval::{ApprovalDecision, ApprovalRecord, TimeoutBehavior};
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::event::{TargetRef, TargetRefKind};
use openspine_schemas::grant::GrantLimits;

use openspine_schemas::selection::{
    SelectionScope, SelectionToken, SelectionTokenType, SelectionVerificationMethod,
};

pub(super) fn sample_grant(task_token: &str) -> TaskGrant {
    let issued_at = Timestamp::now();
    let mut grant = TaskGrant {
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
        allowed_egress_classes: vec![],
        output_channels: vec![],
        limits: GrantLimits {
            max_model_calls: 8,
            max_artifacts: 20,
            max_runtime_seconds: 120,
        },
        task_token: task_token.to_string(),
        root_grant_id: Ulid::nil(),
        parent_grant_id: None,
        mode: openspine_schemas::grant::GrantMode::Live,
        chain: vec![],
        caveat_mac: String::new(),
        thread_id: None,
    };
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");
    grant
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

fn sample_action_request() -> ActionRequest {
    ActionRequest {
        id: Ulid::new(),
        task_grant_id: Ulid::new(),
        action: ActionId::new("email.create_draft"),
        target_ref: Some(TargetRef {
            kind: TargetRefKind::EmailThread,
            id: Some("thread-1".to_string()),
        }),
        payload_ref: Some(ArtifactRef {
            digest: Digest::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
            schema_version: 1,
        }),
        target_digest: Some(Digest::parse(format!("sha256:{}", "b".repeat(64))).unwrap()),
        selection_token_id: None,
        params: std::collections::BTreeMap::new(),
        requested_at: Timestamp::now(),
        schema_version: 1,
    }
}

fn sample_selection_token() -> SelectionToken {
    let now = Timestamp::now();
    SelectionToken {
        id: Ulid::new(),
        schema_version: 1,
        token_type: SelectionTokenType::email_thread_selection(),
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
    // D-047: the persisted grant's task_token is redacted, never round-tripped.
    let mut expected = grant.clone();
    expected.task_token = String::new();
    assert_eq!(back, expected);
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
fn selection_token_round_trips_and_consumes_once() {
    let store = Store::open_in_memory().unwrap();
    let token = sample_selection_token();
    store.insert_selection_token(&token).unwrap();
    let back = store.find_selection_token(token.id).unwrap().unwrap();
    assert_eq!(back, token);

    assert!(store.try_consume_selection_token(token.id).unwrap());
    // A second consumption attempt on the same token must fail — this is
    // the entire single-use enforcement mechanism (PRD §15).
    assert!(!store.try_consume_selection_token(token.id).unwrap());
}

#[test]
fn consuming_an_unknown_selection_token_id_is_a_no_op_failure() {
    let store = Store::open_in_memory().unwrap();
    assert!(!store.try_consume_selection_token(Ulid::new()).unwrap());
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

#[test]
fn action_request_round_trips_by_id() {
    let store = Store::open_in_memory().unwrap();
    let request = sample_action_request();
    store.insert_action_request(&request).unwrap();
    let back = store.find_action_request(request.id).unwrap().unwrap();
    assert_eq!(back, request);
    assert!(store.find_action_request(Ulid::new()).unwrap().is_none());
}

#[test]
fn action_request_consume_is_single_use() {
    // D-044: the callback-driven approval flow must be at-most-once per
    // request, mirroring `try_consume_selection_token`'s guarantee — a
    // second tap on a live "Approve" button (or Telegram redelivering the
    // same `callback_query` update) must not be able to consume the same
    // request twice.
    let store = Store::open_in_memory().unwrap();
    let request = sample_action_request();
    store.insert_action_request(&request).unwrap();

    assert!(store.try_consume_action_request(request.id).unwrap());
    assert!(!store.try_consume_action_request(request.id).unwrap());
}

#[test]
fn consuming_an_unknown_action_request_id_is_a_no_op_failure() {
    assert!(!Store::open_in_memory()
        .unwrap()
        .try_consume_action_request(Ulid::new())
        .unwrap());
}

#[test]
fn opening_a_pre_existing_db_without_the_used_column_is_migrated_in_place() {
    // Simulates a `data/kernel.db` created before `action_requests.used`
    // existed (this table originally shipped without it) — `Store::open`
    // must ALTER the table in place rather than failing, per the module
    // doc comment's warning that `CREATE TABLE IF NOT EXISTS` alone never
    // helps an already-existing file.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("kernel.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE action_requests (id TEXT PRIMARY KEY, request_json TEXT NOT NULL);",
        )
        .unwrap();
    }

    let store = Store::open(&path).unwrap();
    let request = sample_action_request();
    store.insert_action_request(&request).unwrap();
    assert!(store.try_consume_action_request(request.id).unwrap());
    assert!(!store.try_consume_action_request(request.id).unwrap());

    // Re-opening the now-migrated file must also stay a no-op (the
    // "duplicate column name" branch of `apply_ad_hoc_migrations`).
    drop(store);
    assert!(Store::open(&path).is_ok());
}

#[test]
fn find_task_grant_by_token_rejects_the_raw_hash_value() {
    let store = Store::open_in_memory().unwrap();
    let grant = sample_grant("token-b");
    let pending_message_ref = ArtifactRef {
        digest: Digest::parse(format!("sha256:{}", "d".repeat(64))).unwrap(),
        schema_version: 1,
    };
    store
        .insert_task_grant(&grant, &pending_message_ref, 555)
        .unwrap();

    // Proves hashing actually happened, not merely a naive one-way store:
    // looking a grant up by the STORED HASH STRING itself must miss.
    let stored_hash: String = {
        let conn = store.conn.lock();
        conn.query_row("SELECT task_token FROM task_grants LIMIT 1", [], |row| {
            row.get(0)
        })
        .unwrap()
    };
    assert_ne!(stored_hash, "token-b");
    assert!(store
        .find_task_grant_by_token(&stored_hash)
        .unwrap()
        .is_none());
    assert!(store.find_task_grant_by_token("token-b").unwrap().is_some());
}

#[test]
fn persisted_grant_json_contains_no_task_token() {
    let store = Store::open_in_memory().unwrap();
    let grant = sample_grant("super-secret-raw-token-value");
    let pending_message_ref = ArtifactRef {
        digest: Digest::parse(format!("sha256:{}", "e".repeat(64))).unwrap(),
        schema_version: 1,
    };
    store
        .insert_task_grant(&grant, &pending_message_ref, 555)
        .unwrap();

    let grant_json: String = {
        let conn = store.conn.lock();
        conn.query_row("SELECT grant_json FROM task_grants LIMIT 1", [], |row| {
            row.get(0)
        })
        .unwrap()
    };
    assert!(!grant_json.contains("super-secret-raw-token-value"));
}

#[test]
fn sweep_removes_only_grants_expired_more_than_a_day() {
    let store = Store::open_in_memory().unwrap();
    let now = Timestamp::now();
    let old_id = Ulid::new().to_string();
    let recent_id = Ulid::new().to_string();
    {
        let conn = store.conn.lock();
        for (id, expires_at) in [
            (&old_id, now - std::time::Duration::from_secs(25 * 60 * 60)),
            (&recent_id, now - std::time::Duration::from_secs(60 * 60)),
        ] {
            conn.execute(
                "INSERT INTO task_grants (id, task_token, expires_at, grant_json, pending_message_digest, bound_chat_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    id,
                    format!("hash-for-{id}"),
                    expires_at.to_string(),
                    "{}",
                    format!("sha256:{}", "0".repeat(64)),
                    555,
                ],
            )
            .unwrap();
        }
    }

    store.sweep_expired_grants(now).unwrap();

    let remaining: Vec<String> = {
        let conn = store.conn.lock();
        let mut stmt = conn
            .prepare("SELECT id FROM task_grants ORDER BY id")
            .unwrap();
        stmt.query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(remaining, vec![recent_id]);

    // Path 7: sweep_expired_grants is an internal-maintenance non-effect.
    // It must not call gate() or produce any audit events.
    let audit_count = store.count_audit_events_of_kind("action.gated").unwrap();
    assert_eq!(audit_count, 0, "sweep must not produce gate audit events");
}
