use super::*;

#[tokio::test]
async fn captured_value_stays_out_of_audit_metadata() {
    let state = crate::test_support::fixtures::test_state();
    let proof = crate::telegram::VerifiedOwnerContext::test_new();
    assert!(arm(
        &state,
        42,
        state.owner_principal_id,
        &proof,
        SecretMode::Intake,
        "gmail.refresh",
    )
    .expect("arm"));
    assert_eq!(
        capture(&state, 42, "never-in-audit")
            .await
            .expect("capture"),
        Some(CaptureOutcome::Stored(SecretMode::Intake))
    );
    for event in state.store.all_audit_event_jsons().expect("audit rows") {
        assert!(!event.contains("never-in-audit"));
    }
    // The secret must also never land in any kernel-persisted, shell/observer
    // visible key/value surface (the only KV traces are slot names + correlation
    // ids, never the value).
    for (key, value) in state.store.all_kv_for_test() {
        assert!(
            !value.contains("never-in-audit"),
            "secret leaked into kv_state key {key}: {value}"
        );
    }
}

#[tokio::test]
async fn invalid_pending_state_is_consumed_and_never_replayed() {
    let state = crate::test_support::fixtures::test_state();
    state
        .store
        .set_kv(PENDING_KEY, "not-json")
        .expect("set pending");
    assert_eq!(
        capture(&state, 42, "secret").await.expect("capture"),
        Some(CaptureOutcome::Rejected)
    );
    assert_eq!(
        capture(&state, 42, "ordinary").await.expect("capture"),
        None
    );
}

#[tokio::test]
async fn gmail_paired_intake_stages_first_half_then_promotes_on_second() {
    use crate::gmail::GmailConnector;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let token_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "ok",
            "expires_in": 3600,
        })))
        .mount(&token_server)
        .await;

    let gmail = GmailConnector::new(
        "client-id".to_string(),
        String::new(),
        String::new(),
        "owner@example.com".to_string(),
    )
    .with_urls(format!("{}/token", token_server.uri()), token_server.uri());
    let state = crate::test_support::fixtures::test_state_with_gmail(gmail);
    let proof = crate::telegram::VerifiedOwnerContext::test_new();

    // First half: client_secret should be staged, not live.
    assert!(arm(
        &state,
        42,
        state.owner_principal_id,
        &proof,
        SecretMode::Intake,
        "gmail.client_secret",
    )
    .expect("arm"));
    assert_eq!(
        capture(&state, 42, "new-secret").await.expect("capture"),
        Some(CaptureOutcome::Staged(SecretMode::Intake))
    );
    assert!(state
        .secrets
        .get_string("gmail.client_secret")
        .unwrap()
        .is_none());
    assert_eq!(
        state
            .secrets
            .get_string("secret.staged.gmail.client_secret")
            .unwrap(),
        Some("new-secret".to_string())
    );

    // Second half: refresh_token validates the pair and promotes both.
    assert!(arm(
        &state,
        42,
        state.owner_principal_id,
        &proof,
        SecretMode::Intake,
        "gmail.refresh_token",
    )
    .expect("arm"));
    assert_eq!(
        capture(&state, 42, "new-refresh").await.expect("capture"),
        Some(CaptureOutcome::Stored(SecretMode::Intake))
    );
    assert_eq!(
        state.secrets.get_string("gmail.client_secret").unwrap(),
        Some("new-secret".to_string())
    );
    assert_eq!(
        state.secrets.get_string("gmail.refresh_token").unwrap(),
        Some("new-refresh".to_string())
    );
    assert!(state
        .secrets
        .get_string("secret.staged.gmail.client_secret")
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn gmail_paired_intake_works_in_reverse_order() {
    use crate::gmail::GmailConnector;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let token_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "ok",
            "expires_in": 3600,
        })))
        .mount(&token_server)
        .await;

    let gmail = GmailConnector::new(
        "client-id".to_string(),
        String::new(),
        String::new(),
        "owner@example.com".to_string(),
    )
    .with_urls(format!("{}/token", token_server.uri()), token_server.uri());
    let state = crate::test_support::fixtures::test_state_with_gmail(gmail);
    let proof = crate::telegram::VerifiedOwnerContext::test_new();

    // First half: refresh_token.
    assert!(arm(
        &state,
        42,
        state.owner_principal_id,
        &proof,
        SecretMode::Intake,
        "gmail.refresh_token",
    )
    .expect("arm"));
    assert_eq!(
        capture(&state, 42, "r-token").await.expect("capture"),
        Some(CaptureOutcome::Staged(SecretMode::Intake))
    );
    // Second half: client_secret validates and promotes.
    assert!(arm(
        &state,
        42,
        state.owner_principal_id,
        &proof,
        SecretMode::Intake,
        "gmail.client_secret",
    )
    .expect("arm"));
    assert_eq!(
        capture(&state, 42, "c-secret").await.expect("capture"),
        Some(CaptureOutcome::Stored(SecretMode::Intake))
    );
    assert_eq!(
        state.secrets.get_string("gmail.client_secret").unwrap(),
        Some("c-secret".to_string())
    );
    assert_eq!(
        state.secrets.get_string("gmail.refresh_token").unwrap(),
        Some("r-token".to_string())
    );
}

#[tokio::test]
async fn audit_failure_rolls_back_live_credential() {
    let state = crate::test_support::fixtures::test_state();
    let proof = crate::telegram::VerifiedOwnerContext::test_new();

    // Pre-seed the live slot with an old value.
    state.secrets.put("test.slot", b"old-value").unwrap();

    assert!(arm(
        &state,
        42,
        state.owner_principal_id,
        &proof,
        SecretMode::Rotate,
        "test.slot",
    )
    .expect("arm"));

    // Break the audit table so the stored audit fails.
    state.store.break_audit_for_test();

    // Capture must return Err.
    let result = capture(&state, 42, "new-value").await;
    assert!(result.is_err(), "capture must fail on audit failure");

    // The credential must still be the old value, not the new one.
    assert_eq!(
        state.secrets.get_string("test.slot").unwrap(),
        Some("old-value".to_string())
    );
}

#[tokio::test]
async fn audit_failure_rolls_back_paired_promotion() {
    use crate::gmail::GmailConnector;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let token_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "ok",
            "expires_in": 3600,
        })))
        .mount(&token_server)
        .await;

    let gmail = GmailConnector::new(
        "client-id".to_string(),
        String::new(),
        String::new(),
        "owner@example.com".to_string(),
    )
    .with_urls(format!("{}/token", token_server.uri()), token_server.uri());
    let state = crate::test_support::fixtures::test_state_with_gmail(gmail);
    let proof = crate::telegram::VerifiedOwnerContext::test_new();

    // 1. Stage the first half (client_secret).
    assert!(arm(
        &state,
        42,
        state.owner_principal_id,
        &proof,
        SecretMode::Intake,
        "gmail.client_secret",
    )
    .expect("arm"));
    assert_eq!(
        capture(&state, 42, "staged-secret").await.expect("capture"),
        Some(CaptureOutcome::Staged(SecretMode::Intake))
    );

    // 2. Pre-seed the live slots with old values to verify rollback restores them.
    state.secrets.put("gmail.client_secret", b"old-c").unwrap();
    state.secrets.put("gmail.refresh_token", b"old-r").unwrap();

    // Verify staging metadata is in KV.
    let meta_before = state
        .store
        .get_kv("secret.stage.gmail.client_secret")
        .unwrap();
    assert!(meta_before.is_some());

    // 3. Arm the second half (refresh_token).
    assert!(arm(
        &state,
        42,
        state.owner_principal_id,
        &proof,
        SecretMode::Intake,
        "gmail.refresh_token",
    )
    .expect("arm"));

    // 4. Break the audit table so promotion audit fails.
    state.store.break_audit_for_test();

    // 5. Capture should fail.
    let result = capture(&state, 42, "new-refresh").await;
    assert!(result.is_err(), "paired capture must fail on audit failure");

    // 6. Assert live slots were rolled back to pre-seeded old values.
    assert_eq!(
        state.secrets.get_string("gmail.client_secret").unwrap(),
        Some("old-c".to_string())
    );
    assert_eq!(
        state.secrets.get_string("gmail.refresh_token").unwrap(),
        Some("old-r".to_string())
    );

    // 7. Assert staging value was restored.
    assert_eq!(
        state
            .secrets
            .get_string("secret.staged.gmail.client_secret")
            .unwrap(),
        Some("staged-secret".to_string())
    );

    // 8. Assert staging metadata was restored in KV.
    assert_eq!(
        state
            .store
            .get_kv("secret.stage.gmail.client_secret")
            .unwrap(),
        meta_before
    );
}
