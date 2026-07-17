use jiff::Timestamp;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::event::{EventType, Lane, VerificationMethod};

use super::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn update(sender: Option<i64>, text: Option<&str>) -> TelegramUpdate {
    TelegramUpdate {
        update_id: 1,
        chat_id: 555,
        is_private_chat: true,
        sender_user_id: sender,
        text: text.map(str::to_string),
        ..Default::default()
    }
}

#[test]
fn configured_owner_text_message_is_verified() {
    let result = verify_update(&update(Some(42), Some("hello")), 42);
    assert!(matches!(
        result,
        VerifiedUpdate::OwnerMessage {
            chat_id: 555,
            text,
            ..
        } if text == "hello"
    ));
}

#[test]
fn unknown_telegram_user_is_ignored_not_routed() {
    let result = verify_update(&update(Some(99), Some("hello")), 42);
    assert_eq!(
        result,
        VerifiedUpdate::Ignored {
            reason: "unknown_telegram_user"
        }
    );
}

#[test]
fn missing_sender_is_ignored() {
    let result = verify_update(&update(None, Some("hello")), 42);
    assert_eq!(
        result,
        VerifiedUpdate::Ignored {
            reason: "no_sender"
        }
    );
}

#[test]
fn non_text_update_from_owner_is_ignored() {
    let result = verify_update(&update(Some(42), None), 42);
    assert_eq!(
        result,
        VerifiedUpdate::Ignored {
            reason: "non_text_update"
        }
    );
}

#[test]
fn owner_message_in_a_group_chat_is_ignored_not_routed() {
    // The owner is a member of some group and sends a message there —
    // sender id matches, but the chat is not private, so this must
    // never become owner-control routing (the reply would be visible
    // to every other group member).
    let mut group_update = update(Some(42), Some("hello"));
    group_update.is_private_chat = false;
    let result = verify_update(&group_update, 42);
    assert_eq!(
        result,
        VerifiedUpdate::Ignored {
            reason: "owner_message_outside_private_chat"
        }
    );
}

#[test]
fn owner_envelope_is_verified_with_owner_id_match_method() {
    let raw_ref = ArtifactRef {
        digest: openspine_schemas::digest::Digest::parse(format!("sha256:{}", "a".repeat(64)))
            .unwrap(),
        schema_version: 1,
    };
    let envelope = build_owner_envelope(555, raw_ref, Timestamp::now());
    assert!(envelope.verified_source);
    assert_eq!(
        envelope.verification_method,
        VerificationMethod::TelegramOwnerIdMatch
    );
    assert_eq!(envelope.event_type, EventType::TelegramOwnerMessage);
    assert_eq!(envelope.lane, Lane::OwnerControl);
    assert_eq!(envelope.channel_account, "555");
}

#[test]
fn draft_command_extracts_a_well_formed_thread_id() {
    assert_eq!(
        parse_draft_command("/draft abc123DEF-_"),
        Some("abc123DEF-_")
    );
}

#[test]
fn draft_command_trims_surrounding_whitespace() {
    assert_eq!(parse_draft_command("  /draft   thread1  "), Some("thread1"));
}

#[test]
fn draft_command_with_no_id_is_rejected() {
    assert_eq!(parse_draft_command("/draft"), None);
    assert_eq!(parse_draft_command("/draft   "), None);
}

#[test]
fn text_without_the_draft_prefix_is_not_a_draft_command() {
    assert_eq!(parse_draft_command("please draft something"), None);
    assert_eq!(parse_draft_command("hello"), None);
}

#[test]
fn a_prefix_without_a_whitespace_boundary_is_not_a_draft_command() {
    // `/draftabc123` must not be misread as command `/draft` + id
    // `abc123` — no space was actually typed after the token.
    assert_eq!(parse_draft_command("/draftabc123"), None);
    assert_eq!(parse_draft_command("/drafts"), None);
}

#[test]
fn a_thread_id_with_path_or_query_metacharacters_is_rejected() {
    // D-036: this parser is the entire trust boundary for the id that
    // ends up interpolated into the Gmail API request URL — a stray
    // `/`, `?`, `&`, or `#` must never reach the connector.
    assert_eq!(parse_draft_command("/draft foo/bar"), None);
    assert_eq!(parse_draft_command("/draft foo?x=1"), None);
    assert_eq!(parse_draft_command("/draft foo&bar"), None);
    assert_eq!(parse_draft_command("/draft foo#bar"), None);
    assert_eq!(parse_draft_command("/draft ../../etc/passwd"), None);
}

#[test]
fn an_overly_long_thread_id_is_rejected() {
    let too_long = "a".repeat(65);
    assert_eq!(parse_draft_command(&format!("/draft {too_long}")), None);
}

#[tokio::test]
async fn answer_callback_query_is_a_control_plane_ack_with_no_security_effect() {
    // D-055.5: `answer_callback_query` is an internal-maintenance,
    // control-plane ack — it simply echoes the callback id back to
    // Telegram's `answerCallbackQuery` endpoint to stop the owner's
    // tapping spinner. It is never routed through `gate()` and performs
    // no authority-relevant effect; a successful ack is pure UI
    // bookkeeping (§5.8, "no security-relevant effect").
    let tg = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path(
            "/bottest-token/AnswerCallbackQuery",
        ))
        .and(wiremock::matchers::body_string_contains("cb-xyz"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})),
        )
        .expect(1)
        .mount(&tg)
        .await;
    let connector =
        TelegramConnector::with_api_url("test-token".to_string(), tg.uri().parse().unwrap());
    connector.answer_callback_query("cb-xyz").await;
    // Reaching here means `answer_callback_query` fired exactly one
    // `answerCallbackQuery` POST carrying the callback id — a pure
    // control-plane ack. `wiremock` panics on an unmet `.expect(1)` at
    // drop, so the assertion is enforced by the mock's lifetime.
}

#[test]
fn plan_approval_callback_requires_exact_prefix_and_ulid() {
    let id = ulid::Ulid::new();
    assert_eq!(
        parse_approve_plan_callback(&format!("approve_plan:{id}")),
        Some(id)
    );
    assert_eq!(
        parse_approve_plan_callback("approve_draft:01ARZ3NDEKTSV4RRFFQ69G5FAV"),
        None
    );
    assert_eq!(parse_approve_plan_callback("approve_plan:not-a-ulid"), None);
}

#[tokio::test]
async fn rotated_vault_bot_token_is_used_without_restart() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = std::sync::Arc::new(
        crate::secret_store::SecretStore::open(dir.path().join("credentials"), [4; 32])
            .expect("open"),
    );
    store
        .put("telegram.bot_token", b"old-token")
        .expect("seed token");
    let connector = TelegramConnector::new_with_store(
        "old-token".to_string(),
        store.clone(),
        "telegram.bot_token".to_string(),
    );
    assert_eq!(
        connector.current_token_for_test().await.expect("old token"),
        "old-token"
    );
    store
        .put("telegram.bot_token", b"new-token")
        .expect("rotate token");
    assert_eq!(
        connector.current_token_for_test().await.expect("new token"),
        "new-token"
    );
}

#[tokio::test]
async fn invalid_candidate_bot_token_is_rejected_without_replacing_live_bot() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/botcandidate/getMe"))
        .respond_with(ResponseTemplate::new(401).set_body_string("invalid token"))
        .mount(&server)
        .await;
    let connector = TelegramConnector::new("old-token".to_string());
    let api_url: reqwest::Url = format!("{}/", server.uri()).parse().expect("url");
    {
        let mut state = connector.bot.lock();
        state.api_url = Some(api_url.clone());
        state.bot = state.bot.clone().set_api_url(api_url);
    }
    assert!(connector
        .validate_candidate_token_id("candidate")
        .await
        .is_none());
    assert_eq!(
        connector
            .current_token_for_test()
            .await
            .expect("live token"),
        "old-token"
    );
}
