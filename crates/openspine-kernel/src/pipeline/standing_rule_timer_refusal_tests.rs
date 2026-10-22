//! ANO-887: a fired dark-window pending whose briefcase is permanently refused
//! must become terminal instead of re-driving forever.
use super::*;
use crate::api::dispatch_tests::{mint_grant_with_selection_token, OWNER_CHAT_ID};
use crate::store::standing_rules::standing_rule_fingerprint;
use crate::store::standing_rules_tests::manifest;
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::test_state_with_telegram;
use openspine_schemas::action::ActionId;
use openspine_schemas::briefcase::{CounterpartyRef, SectionKind, TaskClass};
use openspine_schemas::digest::canonical_json;
use openspine_schemas::identity::RelationshipKind;
use openspine_schemas::provenance::ProvenanceOrigin;
use openspine_schemas::standing_rule::{BudgetWindow, DarkWindowConfig, DarkWindowDefault};
use rusqlite::params;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn fired_allow_pending_with_legacy_briefcase() -> (AppState, MockServer, String) {
    fired_allow_pending(Breakage::Legacy, false).await
}

#[derive(Clone, Copy)]
enum Breakage {
    Legacy,
    CorruptJson,
}

async fn fired_allow_pending(
    breakage: Breakage,
    first_send_fails: bool,
) -> (AppState, MockServer, String) {
    let server = MockServer::start().await;
    if first_send_fails {
        Mock::given(method("POST"))
            .and(path("/bottest-token/SendMessage"))
            .respond_with(ResponseTemplate::new(500))
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&server)
            .await;
    }
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {"message_id": 1, "date": 0,
                "chat": {"id": OWNER_CHAT_ID, "type": "private"}, "text": "x"}
        })))
        .mount(&server)
        .await;
    let state = test_state_with_telegram(TelegramConnector::with_api_url(
        "test-token".to_string(),
        server.uri().parse().unwrap(),
    ));
    let now = Timestamp::now();
    let manifest = manifest(
        "rule-refused-briefcase",
        "telegram.reply:owner_channel",
        7 * 24 * 3600,
        BudgetWindow {
            max: 5,
            window_secs: 7 * 24 * 3600,
        },
        BudgetWindow {
            max: 5,
            window_secs: 3600,
        },
        Some(DarkWindowConfig {
            timeout_secs: 60,
            default: DarkWindowDefault::Allow,
            max_pending_exceptions: 1,
        }),
    );
    crate::store::standing_rules_tests::activate_or_install_legacy_allow(
        &state.store,
        &manifest,
        now,
    );
    let rule = state
        .store
        .active_standing_rule_for_action(&manifest.action_id, now)
        .unwrap()
        .unwrap();
    let mut grant = mint_grant_with_selection_token(
        &state,
        &["telegram.reply:owner_channel"],
        now + std::time::Duration::from_secs(120),
    )
    .0;
    grant.approval_required_actions = vec![ActionId::new("telegram.reply:owner_channel")];
    grant.seal_root(b"openspine-test-grant-hmac-key-v1");
    let mut stored = grant.clone();
    stored.task_token.clear();
    state.store.with_conn_for_test(|conn| {
        conn.execute(
            "UPDATE task_grants SET grant_json = ?2 WHERE id = ?1",
            params![
                grant.id.to_string(),
                serde_json::to_string(&stored).unwrap()
            ],
        )
        .unwrap();
    });
    // Legacy (#251) briefcase: owner-derived Grant section stamped System.
    let mut briefcase = crate::briefcase::pack_for_task(
        &grant,
        CounterpartyRef::Bound {
            identity_id: ulid::Ulid::new(),
            relationship: RelationshipKind::Owner,
        },
        json!({}),
        TaskClass::Conversation,
        &crate::briefcase::SourcePool { learned: vec![] },
    )
    .unwrap();
    for section in briefcase.sections.iter_mut() {
        if section.kind == SectionKind::Grant {
            section.origin = Some(ProvenanceOrigin::System {});
        }
    }
    state.store.insert_briefcase(grant.id, &briefcase).unwrap();
    if matches!(breakage, Breakage::CorruptJson) {
        state.store.with_conn_for_test(|c| {
            c.execute(
                "UPDATE briefcases SET briefcase_json = '{not json' WHERE task_grant_id = ?1",
                params![grant.id.to_string()],
            )
            .unwrap();
        });
    }
    assert!(state.store.find_briefcase(grant.id).is_err());
    let payload = json!({"text": "fired default"});
    let payload_ref = Some(
        state
            .artifacts
            .put(canonical_json(&payload).as_bytes())
            .unwrap(),
    );
    let surface = crate::test_support::owner_surface_for(&state, OWNER_CHAT_ID);
    let fingerprint = standing_rule_fingerprint(&rule.action_id, grant.id, &surface, &payload_ref);
    let timer_id = state
        .store
        .schedule_standing_rule_dark_window(
            &rule,
            grant.id,
            &surface,
            payload_ref,
            &fingerprint,
            None,
            None,
            now + std::time::Duration::from_secs(60),
            now,
        )
        .unwrap()
        .timer_id()
        .expect("dark window scheduled")
        .to_string();
    (state, server, timer_id)
}

async fn sends(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|r| r.url.path().ends_with("SendMessage"))
        .count()
}

fn count(state: &AppState, kind: &str) -> usize {
    state.store.count_audit_events_of_kind(kind).unwrap()
}

#[tokio::test]
async fn recovery_sweeps_terminate_a_permanently_refused_fired_pending_once() {
    let (state, server, timer_id) = fired_allow_pending_with_legacy_briefcase().await;
    // Claim only (no redispatch): leaves an allowed/none row for recovery.
    let claimed = state
        .store
        .claim_standing_rule_dark_window(
            &timer_id,
            Timestamp::now() + std::time::Duration::from_secs(61),
        )
        .unwrap()
        .expect("claimed");
    let gated_before = count(&state, "action.gated");
    recover_unredriven_pending(&state).await.unwrap();
    let gated_after_first = count(&state, "action.gated");
    recover_unredriven_pending(&state).await.unwrap();
    assert!(state
        .store
        .pending_dark_window_recoverable()
        .unwrap()
        .is_empty());
    let resolution: String = state.store.with_conn_for_test(|c| {
        c.query_row(
            "SELECT resolution FROM standing_rule_pending_actions WHERE pending_id = ?1",
            params![claimed.pending_id],
            |r| r.get(0),
        )
        .unwrap()
    });
    assert_eq!(resolution, "stale");
    // Measure the owner notice's own `action.gated` row directly (kernel
    // `owner.notify` is gated and audited), so the attempt count is exact
    // rather than inferred.
    let sends_before_probe = sends(&server).await;
    let before_notice = count(&state, "action.gated");
    crate::pipeline::notify_owner_best_effort(&state, &state.telegram_owner_surface(), "probe")
        .await;
    let notice_gated = count(&state, "action.gated") - before_notice;
    assert_eq!(notice_gated, 1);
    assert_eq!(
        gated_after_first - gated_before - notice_gated,
        1,
        "exactly one mediation attempt across both sweeps"
    );
    assert_eq!(
        before_notice, gated_after_first,
        "the second sweep must not re-drive the terminal pending"
    );
    assert_eq!(count(&state, "standing_rule.fired_pending_refused"), 1);
    assert_eq!(
        sends_before_probe, 1,
        "owner notified exactly once across both sweeps; no effect"
    );
    assert_eq!(count(&state, "standing_rule.dark_window_admitted"), 0);
    let committed: i64 = state.store.with_conn_for_test(|c| {
        c.query_row("SELECT COUNT(*) FROM standing_rule_usage", [], |r| r.get(0))
            .unwrap()
    });
    assert_eq!(committed, 0, "no budget consumed");
}

#[tokio::test]
async fn timer_claim_path_terminates_a_permanently_refused_fired_pending() {
    let (state, server, timer_id) = fired_allow_pending_with_legacy_briefcase().await;
    claim_and_redispatch(
        &state,
        &timer_id,
        Timestamp::now() + std::time::Duration::from_secs(61),
    )
    .await
    .unwrap();
    recover_unredriven_pending(&state).await.unwrap();
    assert!(state
        .store
        .pending_dark_window_recoverable()
        .unwrap()
        .is_empty());
    assert_eq!(count(&state, "standing_rule.fired_pending_refused"), 1);
    assert_eq!(sends(&server).await, 1);
}

fn dispatch_resource(err: StoreError) -> crate::api::actions::DispatchError {
    crate::api::actions::DispatchError::Resource(anyhow::Error::new(err))
}

#[test]
fn only_the_typed_briefcase_marker_is_permanent() {
    // Unrelated corrupt rows and transient I/O errors are NOT permanent.
    assert!(!is_permanent_refusal(&dispatch_resource(
        StoreError::Serde(serde_json::from_str::<u8>("x").unwrap_err())
    )));
    assert!(!is_permanent_refusal(&dispatch_resource(
        StoreError::LegacyBriefcaseProvenance
    )));
    assert!(!is_permanent_refusal(&dispatch_resource(
        StoreError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            None
        ))
    )));
    assert!(!is_permanent_refusal(
        &crate::api::actions::DispatchError::BadRequest("x".into())
    ));
}

#[tokio::test]
async fn corrupt_briefcase_json_is_terminal_and_cancels_reservations() {
    let (state, server, timer_id) = fired_allow_pending(Breakage::CorruptJson, false).await;
    state
        .store
        .claim_standing_rule_dark_window(
            &timer_id,
            Timestamp::now() + std::time::Duration::from_secs(61),
        )
        .unwrap()
        .expect("claimed");
    recover_unredriven_pending(&state).await.unwrap();
    recover_unredriven_pending(&state).await.unwrap();
    assert!(state
        .store
        .pending_dark_window_recoverable()
        .unwrap()
        .is_empty());
    assert_eq!(count(&state, "standing_rule.fired_pending_refused"), 1);
    assert_eq!(sends(&server).await, 1);
    let live: i64 = state.store.with_conn_for_test(|c| {
        c.query_row(
            "SELECT COUNT(*) FROM standing_rule_usage WHERE status NOT IN ('cancelled')",
            [],
            |r| r.get(0),
        )
        .unwrap()
    });
    assert_eq!(
        live, 0,
        "no reserved/committed usage rows survive the refusal"
    );
}

#[tokio::test]
async fn failed_first_send_is_settled_into_dead_letters_not_resent_by_the_sweep() {
    let (state, server, timer_id) = fired_allow_pending(Breakage::Legacy, true).await;
    state
        .store
        .claim_standing_rule_dark_window(
            &timer_id,
            Timestamp::now() + std::time::Duration::from_secs(61),
        )
        .unwrap()
        .expect("claimed");
    recover_unredriven_pending(&state).await.unwrap(); // terminate; send fails
    assert!(state
        .store
        .refused_pendings_awaiting_owner_notice()
        .unwrap()
        .is_empty());
    recover_unredriven_pending(&state).await.unwrap();
    assert_eq!(sends(&server).await, 1, "the sweep does not resend");
    assert_eq!(state.store.pending_dead_letters().unwrap().len(), 1);
    // The dead-letter queue owns delivery: its worker makes the send.
    state.store.with_conn_for_test(|c| {
        c.execute(
            "UPDATE notify_dead_letters SET next_attempt_at = '1970-01-01T00:00:00Z'",
            [],
        )
        .unwrap();
    });
    crate::failure_surfacing::retry_worker::retry_due_notifications(&state)
        .await
        .unwrap();
    assert_eq!(sends(&server).await, 2, "dead-letter worker delivered it");
    assert!(state.store.pending_dead_letters().unwrap().is_empty());
    assert_eq!(count(&state, "standing_rule.fired_pending_refused"), 1);
}

#[tokio::test]
async fn transient_store_error_leaves_the_pending_retryable() {
    let (state, server, timer_id) = fired_allow_pending(Breakage::Legacy, false).await;
    let claimed = state
        .store
        .claim_standing_rule_dark_window(
            &timer_id,
            Timestamp::now() + std::time::Duration::from_secs(61),
        )
        .unwrap()
        .expect("claimed");
    // A Sqlite error (non-permanent) at the briefcase lookup: the table the
    // lookup reads is unavailable.
    state.store.with_conn_for_test(|c| {
        c.execute_batch("ALTER TABLE briefcases RENAME TO briefcases_gone")
            .unwrap();
    });
    recover_unredriven_pending(&state).await.unwrap();
    recover_unredriven_pending(&state).await.unwrap();
    let (resolution, state_col): (Option<String>, String) = state.store.with_conn_for_test(|c| {
        c.query_row(
            "SELECT resolution, dispatch_state FROM standing_rule_pending_actions WHERE pending_id = ?1",
            params![claimed.pending_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    });
    assert_eq!(resolution.as_deref(), Some("allowed"), "not staled");
    assert_eq!(state_col, "none", "still retryable");
    assert_eq!(count(&state, "standing_rule.fired_pending_refused"), 0);
    assert_eq!(sends(&server).await, 0, "no notice, no effect");
    assert_eq!(
        state.store.pending_dark_window_recoverable().unwrap().len(),
        1
    );
}

#[tokio::test]
async fn permanent_notice_outcome_stops_further_attempts() {
    use crate::pipeline::NotifyOutcome as O;
    let (state, _server, timer_id) = fired_allow_pending(Breakage::Legacy, false).await;
    let claimed = state
        .store
        .claim_standing_rule_dark_window(
            &timer_id,
            Timestamp::now() + std::time::Duration::from_secs(61),
        )
        .unwrap()
        .expect("claimed");
    state
        .store
        .terminate_fired_pending_refused(&claimed.pending_id, "t", Timestamp::now())
        .unwrap();
    settle_notice(&state, &claimed.pending_id, O::GateUnavailable);
    assert_eq!(
        state
            .store
            .refused_pendings_awaiting_owner_notice()
            .unwrap()
            .len(),
        1
    );
    // SendFailed is already in the dead-letter queue: settled.
    settle_notice(&state, &claimed.pending_id, O::SendFailed);
    assert!(state
        .store
        .refused_pendings_awaiting_owner_notice()
        .unwrap()
        .is_empty());
}
