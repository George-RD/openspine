//! Real process recovery with existing authority and historical activity.
use super::*;
use jiff::Timestamp;
use openspine_schemas::action::{ActionId, ActionImplementationId};
use openspine_schemas::briefcase::CounterpartyRef;
use openspine_schemas::delegation_evidence::DelegationEvidence;
use openspine_schemas::digest::digest_of_bytes;
use openspine_schemas::event::{AccountRole, TargetRef, TargetRefKind};
use openspine_schemas::identity::RelationshipKind;
use openspine_schemas::owner_review::*;
use openspine_schemas::resolved_context::{ResolvedActionContext, ResolvedActionContextInput};
use openspine_schemas::reviewed_scope::ReviewedActionScope;
use openspine_schemas::standing_rule::BudgetWindow;
use std::collections::BTreeSet;
use std::time::Duration;
use ulid::Ulid;

#[path = "rollback_gate.rs"]
mod gate;
#[path = "rollback_state.rs"]
mod state;
use gate::{assert_effect_still_requires_approval, seed_historical_activity};
use state::seed_approved_responsibility;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn operator_a_b_a_preserves_revocation_history_and_unknown_effect_fence() {
    let fixture = Fixture {
        root: tempfile::tempdir().unwrap(),
    };
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra"),
        &fixture.root.path().join("candidate"),
    );
    copy(
        &fixture.root.path().join("candidate"),
        &fixture.root.path().join("artifacts/lyra"),
    );
    // These are declared package limits, not clock manipulation or grant edits.
    for base in ["candidate", "artifacts/lyra"] {
        let pack = fixture
            .root
            .path()
            .join(base)
            .join("packs/owner_terminal_basic_pack.yaml");
        let text = fs::read_to_string(&pack).unwrap();
        fs::write(
            pack,
            text.replace("max_runtime_seconds: 120", "max_runtime_seconds: 5"),
        )
        .unwrap();
    }
    // Bootstrap accepts seeds against these already-declared package limits.
    assert_success(&fixture.run(&[
        "init", "--owner", "987654321", "--name", "Tester", "--model", "fixture-model",
    ]));
    let server = super::readiness::provider(&fixture).await;
    let a = fixture.install();
    let source = fixture.root.path().join("revision-b");
    copy(&fixture.root.path().join("candidate"), &source);
    let manifest = source.join("package.yaml");
    fs::write(
        &manifest,
        fs::read_to_string(&manifest)
            .unwrap()
            .replace("version: 2", "version: 3"),
    )
    .unwrap();
    // Different exact package bytes, unchanged runtime-input epoch: this
    // fixture isolates retained-history recovery from the separately tested
    // compatibility-change/reconfirmation lifecycle.
    let document = source.join("README.md");
    let mut bytes = fs::read(&document).unwrap();
    bytes.extend_from_slice(b"\nRevision B operator fixture.\n");
    fs::write(document, bytes).unwrap();
    let output = fixture.run(&["install", "--from", source.to_str().unwrap(), "--json"]);
    assert_success(&output);
    let b: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_ne!(
        a["receipt"]["content_digest"],
        b["receipt"]["content_digest"]
    );

    let (review, command) = seed_approved_responsibility(&fixture);
    assert_eq!(select(&fixture, &id(&a)).code, Some(0));
    startup(&fixture, &a, 1);
    wait_for_grants(&fixture);
    let revoked = fixture.run(&["chat", "--once", &format!("{command} revoke")]);
    assert_success(&revoked);
    assert!(
        String::from_utf8_lossy(&revoked.stdout).contains(": Revoke"),
        "{}",
        String::from_utf8_lossy(&revoked.stdout)
    );
    assert_eq!(status(&fixture), "revoked");
    assert_eq!(
        count(
            &fixture,
            "SELECT COUNT(*) FROM audit_log WHERE kind = 'standing_rule.revoked'"
        ),
        1
    );
    let protected = history(&fixture);
    let keys = files(&fixture.root.path().join("data/keys"));
    let credentials = files(&fixture.root.path().join("data/credentials"));
    let chain = audit_chain(&fixture);

    assert_eq!(select(&fixture, &id(&b)).code, Some(0));
    assert_eq!(selection(&fixture), Some((2, id(&b), 0)));
    assert_eq!(history(&fixture), protected);
    startup(&fixture, &b, 2);
    wait_for_grants(&fixture);
    assert_eq!(select(&fixture, &id(&a)).code, Some(0));
    assert_eq!(selection(&fixture), Some((3, id(&a), 0)));
    assert_eq!(history(&fixture), protected);
    fs::remove_dir_all(fixture.root.path().join("artifacts/lyra")).unwrap();
    startup(&fixture, &a, 3);
    wait_for_grants(&fixture);
    for intent in ["resume", "approve"] {
        let refused = fixture.run(&["chat", "--once", &format!("{command} {intent}")]);
        assert_success(&refused);
        let output = String::from_utf8_lossy(&refused.stdout);
        assert!(output.contains("Review decision refused"), "{output}");
        assert_eq!(status(&fixture), "revoked");
    }
    assert_effect_still_requires_approval(&fixture);
    assert_eq!(history(&fixture), protected);
    assert_eq!(files(&fixture.root.path().join("data/keys")), keys);
    assert_eq!(
        files(&fixture.root.path().join("data/credentials")),
        credentials
    );
    assert_eq!(audit_chain(&fixture)[..chain.len()], chain[..]);
    assert_eq!(
        count(&fixture, "SELECT COUNT(*) FROM package_selection_history"),
        3
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT COUNT(*) FROM owner_reviews WHERE state = 'revoked'"
        ),
        1
    );
    assert!(review.binding_is_valid());
    assert_eq!(server.received_requests().await.unwrap().len(), 3);

    // A lost connector response is real durable work, not a retry opportunity.
    db(&fixture).execute(
        "INSERT INTO pending_draft_writes (id, grant_id, action_request_id, thread_id, request_fingerprint, created_at, state)
         VALUES ('unknown-effect', 'historical-grant', 'historical-request', 'thread-1', 'unknown-fingerprint', '2026-01-01T00:00:00Z', 'pending')", []).unwrap();
    let rows = database_rows(&fixture);
    let blocked = pty::Session::spawn(fixture.command(&["use", &id(&b)])).finish();
    assert_ne!(blocked.code, Some(0), "{}", blocked.output);
    assert!(
        blocked.output.contains("pending_draft_writes"),
        "{}",
        blocked.output
    );
    assert!(!blocked.output.contains(PROMPT));
    assert_eq!(selection(&fixture), Some((3, id(&a), 0)));
    assert_eq!(
        database_rows(&fixture),
        rows,
        "unknown outcome and its fence must remain unchanged"
    );
}

fn startup(fixture: &Fixture, installed: &Value, generation: i64) {
    let output = fixture
        .command(&["chat", "--once", "historical operator activity"])
        .env("RUST_LOG", "info")
        .output()
        .unwrap();
    assert_success(&output);
    let transcript = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        transcript.contains("readiness-provider-reply"),
        "{transcript}"
    );
    assert!(
        transcript.contains("starting from the managed package selection"),
        "{transcript}"
    );
    assert!(
        transcript.contains(installed["receipt"]["content_digest"].as_str().unwrap()),
        "{transcript}"
    );
    assert!(transcript.contains(&id(installed)), "{transcript}");
    assert_eq!(selection(fixture).unwrap().0, generation);
}

fn wait_for_grants(fixture: &Fixture) {
    let grants: Vec<openspine_schemas::grant::TaskGrant> = db(fixture)
        .prepare("SELECT grant_json FROM task_grants")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|s| serde_json::from_str(&s.unwrap()).unwrap())
        .collect();
    if let Some(expiry) = grants.iter().map(|g| g.expires_at).max() {
        let remaining = expiry.duration_since(Timestamp::now());
        if remaining.is_positive() {
            std::thread::sleep(Duration::try_from(remaining).unwrap() + Duration::from_millis(20));
        }
    }
}

fn status(fixture: &Fixture) -> String {
    db(fixture)
        .query_row(
            "SELECT status FROM standing_rules WHERE rule_id = 'operator-rule'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

fn history(fixture: &Fixture) -> Vec<(String, Vec<String>)> {
    let keep = [
        "principals",
        "identities",
        "identity_identifiers",
        "installed_packages",
        "erased_counterparties",
        "standing_rules",
        "standing_rule_usage",
        "owner_reviews",
        "owner_review_standing_rules",
        "approvals",
    ];
    let mut rows: Vec<_> = database_rows(fixture)
        .into_iter()
        .filter(|(name, _)| keep.contains(&name.as_str()))
        .collect();
    let historical: (i64, i64) = db(fixture)
        .query_row(
            "SELECT model_calls, connector_calls FROM daily_spend WHERE day = '2020-01-01'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    rows.push((
        "historical_daily_spend".into(),
        vec![serde_json::to_string(&historical).unwrap()],
    ));
    let consumed: (String, bool) = db(fixture).query_row("SELECT request_json, used FROM action_requests WHERE id = '01ARZ3NDEKTSV4RRFFQ69G5FAZ'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    rows.push((
        "historical_consumed_request".into(),
        vec![serde_json::to_string(&consumed).unwrap()],
    ));
    rows
}
