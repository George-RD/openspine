//! A real first boot must leave no seed approvals blocking package selection.
use super::*;
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn init_chat_install_use_and_restart_accept_seed_provenance() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"content": "first-boot-provider-reply"}}]
        })))
        .expect(2)
        .mount(&server)
        .await;
    let fixture = Fixture {
        root: tempfile::tempdir().unwrap(),
    };
    let base = fixture.root.path().join("artifacts/lyra");
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra"),
        &base,
    );
    // Use the supported package limit, not a production clock override or a
    // ledger rewrite. Thirty seconds leaves the refused census ample headroom.
    let pack = base.join("packs/owner_terminal_basic_pack.yaml");
    let text = fs::read_to_string(&pack).unwrap();
    assert!(text.contains("max_runtime_seconds: 120"));
    fs::write(
        &pack,
        text.replace("max_runtime_seconds: 120", "max_runtime_seconds: 30"),
    )
    .unwrap();
    let initialized = fixture.run(&[
        "init",
        "--owner",
        "987654321",
        "--model",
        "fixture-model",
        "--base-url",
        &server.uri(),
    ]);
    assert_success(&initialized);
    assert_eq!(count(&fixture,
        "SELECT COUNT(*) FROM learned_artifacts WHERE kind = 'workflow' AND compatibility = 'owner_accepted'"), 4);
    let installed = fixture.run(&["install", "lyra", "--json"]);
    assert_success(&installed);
    let installed: Value = serde_json::from_slice(&installed.stdout).unwrap();
    let target = id(&installed);

    let chat = fixture.run(&["chat", "--once", "hello"]);
    assert_success(&chat);
    assert!(String::from_utf8_lossy(&chat.stdout).contains("first-boot-provider-reply"));
    assert_eq!(
        count(
            &fixture,
            "SELECT COUNT(*) FROM action_requests WHERE used = 0"
        ),
        0
    );

    let mut refused = consent_session(&fixture, &target);
    // If a heavily delayed process sees expiry, EOF cancels any unexpected
    // prompt instead of leaving the regression stuck waiting for consent.
    refused.send_eof();
    let refused = refused.finish();
    assert_ne!(refused.code, Some(0), "{}", refused.output);
    assert!(refused.output.contains("task_grants"), "{}", refused.output);
    assert!(
        refused
            .output
            .contains("Wait for active task grants to expire after their TTL"),
        "{}",
        refused.output
    );
    assert!(
        refused.output.contains("120 seconds / 2 minutes"),
        "{}",
        refused.output
    );
    assert!(
        refused.output.contains("300 seconds / 5 minutes"),
        "{}",
        refused.output
    );
    assert!(
        refused.output.contains("notifications: 60 seconds"),
        "{}",
        refused.output
    );
    assert!(!refused.output.contains(PROMPT), "{}", refused.output);
    assert_eq!(selection(&fixture), None);

    let grants: Vec<openspine_schemas::grant::TaskGrant> = db(&fixture)
        .prepare("SELECT grant_json FROM task_grants")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|json| serde_json::from_str(&json.unwrap()).unwrap())
        .collect();
    assert!(!grants.is_empty());
    let expiry = grants
        .iter()
        .map(|grant| {
            assert_eq!(
                grant.expires_at.duration_since(grant.issued_at),
                jiff::SignedDuration::from_secs(30)
            );
            grant.expires_at
        })
        .max()
        .unwrap();
    let remaining = expiry.duration_since(jiff::Timestamp::now());
    if remaining.is_positive() {
        std::thread::sleep(Duration::try_from(remaining).unwrap() + Duration::from_millis(20));
    }
    let rows = protected_rows(&fixture);
    let selected = select(&fixture, &target);
    assert_eq!(selected.code, Some(0), "{}", selected.output);
    assert_eq!(selection(&fixture), Some((1, target.clone(), 0)));
    assert_eq!(protected_rows(&fixture), rows);

    // Exercise the real run() entry and init acceptance, not just the resolver:
    // managed startup must ignore a disappeared configured legacy package.
    fs::remove_dir_all(&base).unwrap();
    assert_success(&fixture.run(&["init", "--model", "fixture-model"]));

    let restarted = fixture
        .command(&["chat", "--once", "hello again"])
        .env("RUST_LOG", "info")
        .output()
        .unwrap();
    assert_success(&restarted);
    let transcript = format!(
        "{}{}",
        String::from_utf8_lossy(&restarted.stdout),
        String::from_utf8_lossy(&restarted.stderr)
    );
    assert!(
        transcript.contains("starting from the managed package selection"),
        "{transcript}"
    );
    assert!(
        transcript.contains("first-boot-provider-reply"),
        "{transcript}"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT COUNT(*) FROM action_requests WHERE used = 0"
        ),
        0
    );
    assert_eq!(count(&fixture,
        "SELECT COUNT(*) FROM learned_artifacts WHERE kind = 'workflow' AND compatibility = 'owner_accepted'"), 4);
}
