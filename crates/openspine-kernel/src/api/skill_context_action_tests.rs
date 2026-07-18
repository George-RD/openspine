use jiff::Timestamp;
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::dispatch_tests::{mint_grant_with_selection_token, OWNER_CHAT_ID};
use super::tests::{post_action, start_server};
use crate::skill::ceremony::install_user_skill;
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::test_state_with_telegram;
use openspine_schemas::skill::{Skill, SkillProvenance, SkillState, SkillVisibility};

async fn post_action_with_skill_token(
    addr: std::net::SocketAddr,
    task_token: &str,
    action: &str,
    payload: Option<Value>,
    skill_context_token_id: &str,
) -> reqwest::Response {
    let mut body = json!({
        "action": action,
        "skill_context_token_id": skill_context_token_id,
    });
    if let Some(payload) = payload {
        body["payload"] = payload;
    }
    reqwest::Client::new()
        .post(format!("http://{addr}/v1/actions"))
        .header("Authorization", format!("Bearer {task_token}"))
        .json(&body)
        .send()
        .await
        .unwrap()
}

fn install_context_skill(state: &crate::pipeline::AppState, id: &str, agent: &str) {
    let body = "Use the granted competence only.".to_string();
    let mut skill = Skill {
        id: id.to_string(),
        schema_version: 1,
        version: 1,
        provenance: SkillProvenance::UserInstalled,
        state: SkillState::PendingReview,
        title: id.to_string(),
        body: body.clone(),
        task_shape: vec!["email_reply".to_string()],
        visibility: SkillVisibility {
            agents: vec![agent.to_string()],
            packs: vec![],
        },
        content_digest: Skill::digest_of_body(&body),
    };
    install_user_skill(
        &state.store,
        state.owner_principal_id,
        &crate::telegram::VerifiedOwnerContext::test_new(),
        &mut skill,
        Timestamp::now(),
    )
    .unwrap();
}

async fn telegram_server(expected_requests: usize) -> (MockServer, TelegramConnector) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 99,
                "date": 0,
                "chat": {"id": OWNER_CHAT_ID, "type": "private"},
                "text": "sent"
            }
        })))
        .expect(expected_requests as u64)
        .mount(&server)
        .await;
    let connector =
        TelegramConnector::with_api_url("test-token".to_string(), server.uri().parse().unwrap());
    (server, connector)
}

#[tokio::test]
async fn http_skill_context_token_has_causal_digest_and_strict_allowed_payload() {
    let (server, connector) = telegram_server(2).await;
    let state = test_state_with_telegram(connector);
    let (grant, _) = mint_grant_with_selection_token(
        &state,
        &["skill.context", "telegram.reply:owner_channel"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );
    install_context_skill(&state, "http_causal_skill", &grant.agent_id.to_string());
    let store = state.store.clone();
    let (addr, handle) = start_server(state).await;

    let context = post_action(addr, &grant.task_token, "skill.context", None).await;
    assert_eq!(context.status(), 200);
    let context_body: Value = context.json().await.unwrap();
    let token = context_body["result"]["skills"][0]["selection_token_id"]
        .as_str()
        .unwrap()
        .to_string();

    let denied = post_action_with_skill_token(
        addr,
        &grant.task_token,
        "email.send",
        Some(json!({"text": "must not escape"})),
        &token,
    )
    .await;
    assert_eq!(denied.status(), 200);
    assert_eq!(
        denied.json::<Value>().await.unwrap()["decision"]["outcome"],
        "deny"
    );
    assert!(store
        .owner_digest_items()
        .unwrap()
        .iter()
        .any(|item| item.summary.contains("skill-derived action denied at gate")));

    let context = post_action(addr, &grant.task_token, "skill.context", None).await;
    let token = context.json::<Value>().await.unwrap()["result"]["skills"][0]["selection_token_id"]
        .as_str()
        .unwrap()
        .to_string();
    let payload = json!({"text": "byte-identical owner reply"});
    let allowed = post_action_with_skill_token(
        addr,
        &grant.task_token,
        "telegram.reply:owner_channel",
        Some(payload.clone()),
        &token,
    )
    .await;
    assert_eq!(allowed.status(), 200);
    assert_eq!(
        allowed.json::<Value>().await.unwrap()["result"]["sent"],
        true
    );
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1].body_json::<Value>().unwrap()["text"],
        payload["text"]
    );

    handle.abort();
}

#[tokio::test]
async fn http_tokenless_denial_reports_bounded_contextual_notice_without_consuming() {
    let (_server, connector) = telegram_server(1).await;
    let state = test_state_with_telegram(connector);
    let (grant, _) = mint_grant_with_selection_token(
        &state,
        &["skill.context"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );
    install_context_skill(&state, "http_contextual_skill", &grant.agent_id.to_string());
    let store = state.store.clone();
    let (addr, handle) = start_server(state).await;

    for _ in 0..2 {
        let response = post_action(addr, &grant.task_token, "skill.context", None).await;
        assert_eq!(response.status(), 200);
    }
    let denied = post_action(addr, &grant.task_token, "email.send", None).await;
    assert_eq!(denied.status(), 200);
    assert_eq!(
        denied.json::<Value>().await.unwrap()["decision"]["outcome"],
        "deny"
    );

    let items = store.owner_digest_items().unwrap();
    let item = items
        .iter()
        .find(|item| item.summary.contains("active skills"))
        .expect("tokenless denial must create contextual digest item");
    assert!(!item.summary.contains("skill-derived action denied at gate"));
    assert!(item.summary.contains("http_contextual_skill v1"));
    assert_eq!(
        crate::store::skill_read_queries::live_skill_context_selections(&store, grant.id)
            .unwrap()
            .len(),
        2,
        "contextual attribution must not consume live selections"
    );

    handle.abort();
}

#[tokio::test]
async fn http_skill_context_token_concurrent_submit_has_one_winner() {
    let (server, connector) = telegram_server(1).await;
    let state = test_state_with_telegram(connector);
    let (grant, _) = mint_grant_with_selection_token(
        &state,
        &["skill.context", "telegram.reply:owner_channel"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );
    install_context_skill(&state, "http_concurrent_skill", &grant.agent_id.to_string());
    let store = state.store.clone();
    let (addr, handle) = start_server(state).await;
    let context = post_action(addr, &grant.task_token, "skill.context", None).await;
    let token = context.json::<Value>().await.unwrap()["result"]["skills"][0]["selection_token_id"]
        .as_str()
        .unwrap()
        .to_string();

    let (first, second) = tokio::join!(
        post_action_with_skill_token(
            addr,
            &grant.task_token,
            "telegram.reply:owner_channel",
            Some(json!({"text": "one"})),
            &token,
        ),
        post_action_with_skill_token(
            addr,
            &grant.task_token,
            "telegram.reply:owner_channel",
            Some(json!({"text": "two"})),
            &token,
        ),
    );
    let statuses = [first.status(), second.status()];
    assert!(statuses.contains(&reqwest::StatusCode::OK));
    assert!(statuses.contains(&reqwest::StatusCode::BAD_REQUEST));
    assert_eq!(
        crate::store::skill_read_queries::live_skill_context_selections(&store, grant.id)
            .unwrap()
            .len(),
        1
    );
    assert!(
        crate::store::skill_read_queries::find_live_skill_context_selection(
            &store,
            token.parse().unwrap(),
            grant.id,
        )
        .unwrap()
        .is_none()
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    handle.abort();
}

#[tokio::test]
async fn http_consumed_causal_token_still_contextualizes_later_tokenless_denial() {
    let (server, connector) = telegram_server(2).await;
    let state = test_state_with_telegram(connector);
    let (grant, _) = mint_grant_with_selection_token(
        &state,
        &["skill.context", "telegram.reply:owner_channel"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );
    install_context_skill(&state, "http_lifetime_skill", &grant.agent_id.to_string());
    let store = state.store.clone();
    let (addr, handle) = start_server(state).await;

    let context = post_action(addr, &grant.task_token, "skill.context", None).await;
    let token = context.json::<Value>().await.unwrap()["result"]["skills"][0]["selection_token_id"]
        .as_str()
        .unwrap()
        .to_string();
    let allowed = post_action_with_skill_token(
        addr,
        &grant.task_token,
        "telegram.reply:owner_channel",
        Some(json!({"text": "benign first action"})),
        &token,
    )
    .await;
    assert_eq!(allowed.status(), 200);

    let denied = post_action(addr, &grant.task_token, "email.send", None).await;
    assert_eq!(denied.status(), 200);
    assert_eq!(
        denied.json::<Value>().await.unwrap()["decision"]["outcome"],
        "deny"
    );
    assert!(store
        .owner_digest_items()
        .unwrap()
        .iter()
        .any(|item| item.summary.contains("active skills")));
    assert_eq!(
        crate::store::skill_read_queries::live_skill_context_selections(&store, grant.id)
            .unwrap()
            .len(),
        1
    );

    handle.abort();
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}
