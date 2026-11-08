//! Readiness must diagnose the same committed base as runtime startup.
use super::*;

pub(super) async fn provider(fixture: &Fixture) -> wiremock::MockServer {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"content": "readiness-provider-reply"}}]
        })))
        .mount(&server)
        .await;
    let config = fixture.root.path().join("openspine.yaml");
    let text = fs::read_to_string(&config).unwrap();
    fs::write(
        config,
        text.replace("http://127.0.0.1:11434", &server.uri()),
    )
    .unwrap();
    server
}

fn report(fixture: &Fixture) -> Output {
    fixture.run(&["setup", "--check"])
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn readiness_distinguishes_legacy_candidates_and_committed_selection_without_mutation() {
    let fixture = Fixture::new();
    let _server = provider(&fixture).await;
    let installed = fixture.install();
    let target = id(&installed);
    let before = database_rows(&fixture);
    let legacy = report(&fixture);
    assert_success(&legacy);
    let rendered = text(&legacy);
    assert!(rendered.contains("legacy configured base"), "{rendered}");
    assert!(
        rendered.contains("installed-inactive candidate"),
        "{rendered}"
    );
    assert!(rendered.contains(&target), "{rendered}");
    assert!(rendered.contains("local-unverified"), "{rendered}");
    assert_eq!(database_rows(&fixture), before);

    assert_eq!(select(&fixture, &target).code, Some(0));
    fs::remove_dir_all(fixture.root.path().join("artifacts/lyra")).unwrap();
    let before = database_rows(&fixture);
    let selected = report(&fixture);
    assert_success(&selected);
    let rendered = text(&selected);
    assert!(rendered.contains("committed selection"), "{rendered}");
    assert!(rendered.contains(&target), "{rendered}");
    assert!(rendered.contains("lyra_dir ignored"), "{rendered}");
    assert_eq!(database_rows(&fixture), before);

    fs::remove_dir_all(fixture.object(&installed)).unwrap();
    let missing = report(&fixture);
    assert!(!missing.status.success());
    let rendered = text(&missing);
    assert!(
        rendered.contains("unavailable selected package"),
        "{rendered}"
    );
    assert!(rendered.contains(&target), "{rendered}");
    assert!(!rendered.contains("set `lyra_dir"), "{rendered}");
    assert_eq!(database_rows(&fixture), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn readiness_refuses_a_corrupt_selection_ledger_instead_of_legacy_fallback() {
    let fixture = Fixture::new();
    let _server = provider(&fixture).await;
    let installed = fixture.install();
    assert_eq!(select(&fixture, &id(&installed)).code, Some(0));
    db(&fixture)
        .execute("DELETE FROM package_selection", [])
        .unwrap();
    let before = database_rows(&fixture);
    let output = report(&fixture);
    assert!(!output.status.success());
    assert!(
        text(&output).contains("selection-ledger-invalid"),
        "{}",
        text(&output)
    );
    assert_eq!(database_rows(&fixture), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_different_product_candidate_never_changes_readiness_or_selection() {
    let fixture = Fixture::new();
    let _server = provider(&fixture).await;
    let installed = fixture.install();
    assert_eq!(select(&fixture, &id(&installed)).code, Some(0));
    let manifest = fixture.root.path().join("candidate/package.yaml");
    let source = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        source.replacen("id: lyra", "id: other_product", 1),
    )
    .unwrap();
    let other = fixture.install();
    let before = database_rows(&fixture);
    let output = report(&fixture);
    assert_success(&output);
    let rendered = text(&output);
    assert!(rendered.contains("committed selection"), "{rendered}");
    assert!(rendered.contains("other_product"), "{rendered}");
    assert!(rendered.contains("different-product"), "{rendered}");
    assert!(rendered.contains(&id(&other)), "{rendered}");
    assert_eq!(database_rows(&fixture), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn readiness_does_not_acquire_the_lifetime_lock_held_by_another_process() {
    let fixture = Fixture::new();
    let _server = provider(&fixture).await;
    fixture.install();
    let candidate = install_variant(&fixture, 3);
    let mut session = consent_session(&fixture, &id(&candidate));
    session.wait_for(PROMPT);
    // The use process holds the same lifetime lock as a running service.
    let before = database_rows(&fixture);
    let output = report(&fixture);
    assert_success(&output);
    assert!(text(&output).contains("legacy configured base"));
    assert_eq!(database_rows(&fixture), before);
    session.send_eof();
    assert_ne!(session.finish().code, Some(0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fresh_setup_declining_seed_consent_remains_legacy_and_ready_without_a_database() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data": [{"id": "fixture-model"}]})),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"choices": [{"message": {"content": "setup-provider-reply"}}]}),
        ))
        .mount(&server)
        .await;
    let fixture = Fixture {
        root: tempfile::tempdir().unwrap(),
    };
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra"),
        &fixture.root.path().join("artifacts/lyra"),
    );
    let mut session = pty::Session::spawn(fixture.command(&["setup"]));
    session.wait_for("Write a starter configuration there?");
    session.send_line("y");
    session.wait_for("Your Telegram user id");
    session.send_line("987654321");
    session.wait_for("Your name");
    session.send_line("Owner");
    session.wait_for("Model endpoint");
    session.send_line(&server.uri());
    session.wait_for("Model [1]");
    session.send_line("fixture-model");
    session.wait_for("Accept the four shipped workflows as this install's owner?");
    session.send_line("n");
    session.wait_for("choose [5]");
    session.send_line("5");
    let finished = session.finish();
    assert_eq!(finished.code, Some(0), "{}", finished.output);
    assert!(finished.output.contains("workflows not accepted"));
    assert!(finished.output.contains("legacy configured base"));
    assert!(!finished.output.contains("ledger-unavailable"));
    assert!(!finished.output.contains("migrate an older ledger"));
    assert!(fixture.root.path().join("data").is_dir());
    assert!(!fixture.root.path().join("data/kernel.db").exists());
    assert!(!fixture.root.path().join("data/packages").exists());
    assert_success(&report(&fixture));
    assert!(!fixture.root.path().join("data/kernel.db").exists());
}
