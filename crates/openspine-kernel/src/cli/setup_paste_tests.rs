//! Manual-paste parsing tests for `setup::finish`: bare codes, the
//! Anthropic `<code>#<state>` pair, and the full redirected callback URL a
//! Codex login leaves behind. Split from `setup_tests.rs` for the
//! 500-line module gate.

use super::*;
use crate::secret_store::SecretStore;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TEST_CLIENT_ID: &str = "test-client-id";

fn vault(tag: &str) -> (tempfile::TempDir, SecretStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = SecretStore::open(dir.path().join(tag), [21; 32]).expect("open");
    (dir, store)
}

/// Anthropic's manual paste hands back `<code>#<state>`, which is what an owner
/// copies out of the browser when no loopback listener is reachable. That is the
/// primary path for an SSH install, so the split is not a nicety. State
/// round-trips verbatim, so the pasted state is the one this flow minted.
#[tokio::test]
async fn a_pasted_code_carrying_its_state_is_split_and_both_are_sent() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "access-value",
            "refresh_token": "refresh-value",
            "expires_in": 3600
        })))
        .mount(&server)
        .await;
    let (_dir, store) = vault("credentials");
    let auth = begin_with_client_id("anthropic", 54545, TEST_CLIENT_ID).expect("begin");
    let state = auth.state().to_string();

    finish(
        &auth,
        &format!("the-code#{state}"),
        &store,
        &reqwest::Client::new(),
        Some(&format!("{}/token", server.uri())),
    )
    .await
    .expect("finish");

    let request = &server.received_requests().await.expect("requests")[0];
    let body: serde_json::Value = serde_json::from_slice(&request.body).expect("json body");
    assert_eq!(body["code"].as_str(), Some("the-code"));
    assert_eq!(body["state"].as_str(), Some(state.as_str()));
    // JSON, not form encoding: the token endpoint rejects a form body.
    assert_eq!(
        request
            .headers
            .get("content-type")
            .map(|v| v.to_str().unwrap()),
        Some("application/json")
    );
}

/// A pasted state that is not this flow's state is a corrupted paste or a
/// response minted for a different authorization; it must be refused before
/// any token endpoint sees it.
#[tokio::test]
async fn a_pasted_state_from_another_flow_is_refused() {
    let (_dir, store) = vault("credentials");
    let auth = begin_with_client_id("anthropic", 54545, TEST_CLIENT_ID).expect("begin");

    let error = finish(
        &auth,
        "the-code#state-from-some-other-flow",
        &store,
        &reqwest::Client::new(),
        Some("http://127.0.0.1:9/token"),
    )
    .await
    .expect_err("must refuse");

    assert!(error.to_string().contains("state mismatch"), "{error}");
}

/// A Codex login on a headless box leaves exactly one artifact: the full
/// redirected URL in the other browser's address bar. Pasting it whole must
/// work, and its state must belong to this flow.
#[tokio::test]
async fn pasted_redirect_url_yields_code_and_checks_state() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "access-value",
            "refresh_token": "refresh-value",
            "expires_in": 3600
        })))
        .mount(&server)
        .await;
    let (_dir, store) = vault("credentials");
    let auth = begin_with_client_id("anthropic", 54545, TEST_CLIENT_ID).expect("begin");
    let state = auth.state().to_string();

    finish(
        &auth,
        &format!("http://localhost:1455/auth/callback?code=url%2Dcode&state={state}"),
        &store,
        &reqwest::Client::new(),
        Some(&format!("{}/token", server.uri())),
    )
    .await
    .expect("finish");

    let request = &server.received_requests().await.expect("requests")[0];
    let body: serde_json::Value = serde_json::from_slice(&request.body).expect("json body");
    assert_eq!(
        body["code"].as_str(),
        Some("url-code"),
        "the code must be extracted and url-decoded from the pasted URL"
    );

    let mismatched = finish(
        &auth,
        "http://localhost:1455/auth/callback?code=url-code&state=someone-elses-state",
        &store,
        &reqwest::Client::new(),
        Some(&format!("{}/token", server.uri())),
    )
    .await
    .expect_err("a foreign state must be refused");
    assert!(
        mismatched.to_string().contains("state mismatch"),
        "{mismatched}"
    );
}

/// A code with no `#` keeps the state the authorization generated.
#[tokio::test]
async fn a_bare_pasted_code_uses_the_authorization_state() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "a",
            "refresh_token": "r",
            "expires_in": 3600
        })))
        .mount(&server)
        .await;
    let (_dir, store) = vault("credentials");
    let auth = begin_with_client_id("anthropic", 54545, TEST_CLIENT_ID).expect("begin");
    let expected_state = auth.state().to_string();

    finish(
        &auth,
        "plain-code",
        &store,
        &reqwest::Client::new(),
        Some(&format!("{}/token", server.uri())),
    )
    .await
    .expect("finish");

    let request = &server.received_requests().await.expect("requests")[0];
    let body: serde_json::Value = serde_json::from_slice(&request.body).expect("json body");
    assert_eq!(body["code"].as_str(), Some("plain-code"));
    assert_eq!(body["state"].as_str(), Some(expected_state.as_str()));
}
