//! End-to-end command tests for the skill artifact class (AD-040/AD-041):
//! `/skill install <complete-skill-json>` parses the JSON as the sole
//! artifact source (deny_unknown_fields + required fields enforced by serde,
//! payload size capped), and `/promote <id> <version>` records an
//! owner-principal- and digest-bound preview that the subsequent
//! approve/reject must consume atomically.
//!
//! These exercise the real `handle_owner_update` command router, not the
//! ceremony fns directly, so the production preview-binding path is covered.

use jiff::Timestamp;
use openspine_schemas::skill::{Skill, SkillProvenance, SkillState, SkillVisibility};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::skill::ceremony::install_mined_skill;
use crate::store::skill_store::get_skill;
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::test_state_with_telegram;

const MAX_PAYLOAD: usize = 64 * 1024;

/// Build a complete, schema-valid `Skill` JSON for `/skill install`.
/// `content_digest` is placeholder — the ceremony recomputes it from
/// `body` and overrides `provenance` to `UserInstalled`.
fn skill_json(id: &str, body: &str) -> String {
    format!(
        "{{\"id\":\"{id}\",\"schema_version\":1,\"version\":1,\
\"provenance\":\"user_installed\",\"state\":\"pending_review\",\
\"title\":\"{id}\",\"body\":\"{body}\",\"task_shape\":[\"email_reply\"],\
\"visibility\":{{\"agents\":[\"email_reply_drafter\"],\"packs\":[]}},\
\"content_digest\":\"sha256:0000000000000000000000000000000000000000000000000000000000000000\"}}"
    )
}

fn mined_skill(id: &str) -> Skill {
    let visibility = SkillVisibility {
        agents: vec!["email_reply_drafter".to_string()],
        packs: vec![],
    };
    let body = format!("competence text for {id}");
    let digest = Skill::digest_of_body(&body);
    Skill {
        id: id.to_string(),
        schema_version: 1,
        version: 1,
        provenance: SkillProvenance::MinerDistilled,
        state: SkillState::PendingReview,
        title: format!("skill {id}"),
        body,
        task_shape: vec!["email_reply".to_string()],
        visibility,
        content_digest: digest,
    }
}

#[tokio::test]
async fn skill_install_parses_complete_json_and_installs() {
    let state = test_state();
    let json = skill_json("cmd_skill", "do the thing");
    let update = owner_update(&format!("/skill install {json}"));
    handle_owner_update(&state, &update).await.unwrap();

    let stored = get_skill(&state.store, "cmd_skill", 1).unwrap().unwrap();
    assert_eq!(stored.state, SkillState::Installed);
    // Ceremony overrides provenance + recomputes the digest from body.
    assert_eq!(stored.provenance, SkillProvenance::UserInstalled);
    assert_eq!(stored.content_digest, Skill::digest_of_body("do the thing"));
}

#[tokio::test]
async fn skill_install_rejects_empty_payload() {
    let state = test_state();
    let update = owner_update("/skill install ");
    handle_owner_update(&state, &update).await.unwrap();

    assert!(
        get_skill(&state.store, "cmd_skill", 1).unwrap().is_none(),
        "empty payload must not install anything"
    );
}

#[tokio::test]
async fn skill_install_rejects_unknown_field() {
    let state = test_state();
    // `extra_field` is not part of `Skill` — deny_unknown_fields must
    // reject the payload rather than silently dropping it.
    let json = "{\"id\":\"extra_skill\",\"schema_version\":1,\"version\":1,\
\"provenance\":\"user_installed\",\"state\":\"pending_review\",\
\"title\":\"x\",\"body\":\"b\",\"task_shape\":[],\"visibility\":{\"agents\":[],\"packs\":[]},\
\"content_digest\":\"sha256:0000000000000000000000000000000000000000000000000000000000000000\",\
\"extra_field\":\"evil\"}"
        .to_string();
    let update = owner_update(&format!("/skill install {json}"));
    handle_owner_update(&state, &update).await.unwrap();

    assert!(
        get_skill(&state.store, "extra_skill", 1).unwrap().is_none(),
        "unknown field must be rejected, not installed"
    );
}

#[tokio::test]
async fn skill_install_rejects_missing_required_field() {
    let state = test_state();
    // Omit `body` (required) — serde must reject.
    let json = "{\"id\":\"noskill\",\"schema_version\":1,\"version\":1,\
\"provenance\":\"user_installed\",\"state\":\"pending_review\",\
\"title\":\"x\",\"task_shape\":[],\"visibility\":{\"agents\":[],\"packs\":[]},\
\"content_digest\":\"sha256:0000000000000000000000000000000000000000000000000000000000000000\"}";
    let update = owner_update(&format!("/skill install {json}"));
    handle_owner_update(&state, &update).await.unwrap();

    assert!(
        get_skill(&state.store, "noskill", 1).unwrap().is_none(),
        "missing required field must be rejected"
    );
}

#[tokio::test]
async fn skill_install_rejects_oversize_payload() {
    let state = test_state();
    let big = "a".repeat(MAX_PAYLOAD + 1);
    let json = skill_json("big_skill", &big);
    assert!(json.len() > MAX_PAYLOAD);
    let update = owner_update(&format!("/skill install {json}"));
    handle_owner_update(&state, &update).await.unwrap();

    assert!(
        get_skill(&state.store, "big_skill", 1).unwrap().is_none(),
        "oversize payload must be rejected"
    );
}

#[tokio::test]
async fn promote_command_records_preview_then_approve_consumes() {
    // Mock Telegram so the preview send succeeds (preview is persisted only
    // after confirmed delivery).
    let server = MockServer::start().await;
    let token = "test-token";
    Mock::given(method("POST"))
        .and(path(format!("/bot{token}/SendMessage")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 99,
                "date": 0,
                "chat": {"id": 555, "type": "private"},
                "text": "sent"
            }
        })))
        .expect(2)
        .mount(&server)
        .await;

    let connector =
        TelegramConnector::with_api_url(token.to_string(), server.uri().parse().unwrap());
    let state = test_state_with_telegram(connector);

    let mut skill = mined_skill("promo_skill");
    install_mined_skill(&state.store, &mut skill, Timestamp::now()).unwrap();
    assert_eq!(
        get_skill(&state.store, "promo_skill", 1)
            .unwrap()
            .unwrap()
            .state,
        SkillState::PendingReview
    );

    // Owner previews -> the command sends the preview text and records the
    // owner-principal- and digest-bound preview (only after confirmed send).
    // The shown text must include provenance and content-level diff.
    let result = handle_owner_update(&state, &owner_update("/promote promo_skill 1"))
        .await
        .unwrap();
    assert!(result.is_none(), "promote preview must consume the update");

    // Verify the preview text sent to Telegram includes provenance and diff.
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1, "exactly one Telegram send for preview");
    let body = String::from_utf8_lossy(&requests[0].body).to_string();
    assert!(
        body.contains("provenance"),
        "preview text must include provenance: {body}"
    );
    assert!(
        body.contains("digest"),
        "preview text must include digest: {body}"
    );
    assert!(
        body.contains("prior") || body.contains("first version"),
        "preview text must include prior version info: {body}"
    );

    // Owner approves -> the decision consumes the exact preview
    // record (digest + principal bound, atomic) and lands the skill.
    handle_owner_update(&state, &owner_update("/promote promo_skill 1 approve"))
        .await
        .unwrap();

    assert_eq!(
        get_skill(&state.store, "promo_skill", 1)
            .unwrap()
            .unwrap()
            .state,
        SkillState::Installed
    );
}

#[tokio::test]
async fn promote_approve_without_preview_fails_closed() {
    let state = test_state();
    let mut skill = mined_skill("nopreview");
    install_mined_skill(&state.store, &mut skill, Timestamp::now()).unwrap();

    // Approve directly, with NO prior preview step. The decision must
    // fail closed: no matching unconsumed preview record exists, so the
    // skill stays PendingReview.
    handle_owner_update(&state, &owner_update("/promote nopreview 1 approve"))
        .await
        .unwrap();

    assert_eq!(
        get_skill(&state.store, "nopreview", 1)
            .unwrap()
            .unwrap()
            .state,
        SkillState::PendingReview,
        "approve without a prior preview must be denied"
    );
}

#[tokio::test]
async fn promote_failed_notify_rejects_approve() {
    // Mock Telegram to FAIL the preview send.
    let server = MockServer::start().await;
    let token = "test-token";
    Mock::given(method("POST"))
        .and(path(format!("/bot{token}/SendMessage")))
        .respond_with(ResponseTemplate::new(500))
        .expect(3)
        .mount(&server)
        .await;

    let connector =
        TelegramConnector::with_api_url(token.to_string(), server.uri().parse().unwrap());
    let state = test_state_with_telegram(connector);

    let mut skill = mined_skill("fail_notify");
    install_mined_skill(&state.store, &mut skill, Timestamp::now()).unwrap();

    // Preview with failed Telegram send -> preview NOT persisted.
    let result = handle_owner_update(&state, &owner_update("/promote fail_notify 1"))
        .await
        .unwrap();
    assert!(result.is_none(), "promote preview must consume the update");

    // Approve must fail: no consumable preview record exists.
    handle_owner_update(&state, &owner_update("/promote fail_notify 1 approve"))
        .await
        .unwrap();

    assert_eq!(
        get_skill(&state.store, "fail_notify", 1)
            .unwrap()
            .unwrap()
            .state,
        SkillState::PendingReview,
        "approve after failed notify must be denied (preview not persisted)"
    );
}

#[tokio::test]
async fn skill_install_bare_command_is_usage_not_owner_message() {
    let state = test_state();
    // Bare `/skill install` (no payload, no trailing space) must be
    // treated as a malformed install command (usage), NOT routed as
    // an ordinary owner message that would compose a grant.
    let update = owner_update("/skill install");
    let out = handle_owner_update(&state, &update).await.unwrap();
    assert!(
        out.is_none(),
        "bare /skill install must have no side effect"
    );
    assert!(
        get_skill(&state.store, "install", 1).unwrap().is_none(),
        "bare command must not install anything"
    );
}
