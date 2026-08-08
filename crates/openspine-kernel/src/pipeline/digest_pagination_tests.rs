//! Unit tests for owner-digest pagination. Extracted to a sibling so
//! `digest_pagination.rs` is not parked on the 500-line cap.

use super::{detail_pages, handle_detail_command, render_page, TELEGRAM_DIGEST_CAP};
use crate::failure_surfacing::{batch_failure, FailureClass};
use crate::store::failure_surfacing_types::{DigestItem, MAX_DIGEST_SUMMARY_CHARS};
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::{test_state, test_state_with_telegram};
use jiff::Timestamp;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::Digest;
use serde_json::json;
use ulid::Ulid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn item(summary: &str) -> DigestItem {
    DigestItem {
        id: Ulid::new(),
        ts: Timestamp::now(),
        class: "connector".to_string(),
        summary: summary.to_string(),
        text_ref: None,
        resolved: false,
    }
}

#[test]
fn admits_more_than_three_tiny_items() {
    let items = (0..8)
        .map(|i| item(&format!("tiny-{i}")))
        .collect::<Vec<_>>();
    let (page, ids) = render_page(&items);
    assert_eq!(ids.len(), 8);
    assert!(page.len() <= TELEGRAM_DIGEST_CAP);
}

#[test]
fn drains_aggregate_across_pages_without_duplicates() {
    let mut pending = (0..9)
        .map(|i| item(&format!("{}-{i}", "x".repeat(1_500))))
        .collect::<Vec<_>>();
    let all_ids = pending
        .iter()
        .map(|item| item.id)
        .collect::<std::collections::HashSet<_>>();
    let mut delivered = std::collections::HashSet::new();
    for _ in 0..10 {
        if pending.is_empty() {
            break;
        }
        let (page, ids) = render_page(&pending);
        assert!(page.len() <= TELEGRAM_DIGEST_CAP);
        assert!(!ids.is_empty());
        for id in &ids {
            assert!(delivered.insert(*id));
        }
        pending.retain(|item| !ids.contains(&item.id));
    }
    assert_eq!(delivered, all_ids);
}

#[test]
fn oversized_single_item_is_bounded_and_delivered_by_reference() {
    // Bypass the store's summary bound to exercise the render-time cap:
    // an unbounded summary must never overflow the page.
    let oversized = DigestItem {
        id: Ulid::new(),
        ts: Timestamp::now(),
        class: "connector".to_string(),
        summary: "x".repeat(TELEGRAM_DIGEST_CAP + 100),
        text_ref: None,
        resolved: false,
    };
    let id = oversized.id;
    let (page, ids) = render_page(&[oversized]);
    assert!(page.len() <= TELEGRAM_DIGEST_CAP);
    assert_eq!(ids, vec![id]);
    assert!(page.contains(&id.to_string()));
    assert!(page.contains("protected detail"));
}

#[test]
fn long_detail_is_bounded_in_summary_and_full_via_ref() {
    let state = test_state();
    let detail = "x".repeat(5_000);
    batch_failure(
        &state,
        FailureClass::Connector,
        "connector failure",
        &detail,
    )
    .expect("batch");
    let items = state.store.owner_digest_items().expect("digest");
    assert_eq!(items.len(), 1);
    // SQLite keeps only the bounded, non-sensitive summary.
    assert!(
        items[0].summary.chars().count() <= MAX_DIGEST_SUMMARY_CHARS,
        "summary must be bounded: {}",
        items[0].summary.len()
    );
    assert!(!items[0].summary.contains(&"x".repeat(5_000)));
    // The full sensitive detail is retrievable only via the encrypted ref.
    let ref_str = items[0]
        .text_ref
        .clone()
        .expect("encrypted text_ref present");
    let digest = Digest::parse(&ref_str).expect("valid ref");
    let bytes = state
        .artifacts
        .get(&ArtifactRef {
            digest,
            schema_version: 1,
        })
        .expect("artifact readable");
    assert_eq!(bytes, detail.as_bytes());
}

#[test]
fn detail_pages_reconstruct_every_byte_on_utf8_boundaries() {
    let detail = format!("{}é", "x".repeat(10_000));
    let pages = detail_pages(&detail);
    assert!(pages.len() > 1);
    assert_eq!(pages.concat(), detail);
    assert!(pages
        .iter()
        .all(|page| page.len() <= TELEGRAM_DIGEST_CAP - 128));
}

const TEST_GRANT_KEY: &str = "openspine-test-grant-hmac-key-v1";

async fn mount_send_ok(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "date": 0,
                "chat": {"id": 555, "type": "private"},
                "text": "sent"
            }
        })))
        .mount(server)
        .await;
}

fn telegram(server: &MockServer) -> TelegramConnector {
    TelegramConnector::with_api_url("test-token".to_string(), server.uri().parse().unwrap())
}

#[tokio::test]
async fn canonical_unavailable_marker_view_does_not_grow_items_or_audits() {
    std::env::set_var("OPENSPINE_GRANT_HMAC_KEY", TEST_GRANT_KEY);
    let server = MockServer::start().await;
    mount_send_ok(&server).await;
    let state = test_state_with_telegram(telegram(&server));

    let marker_id = state
        .store
        .record_unavailable_failure("resource")
        .expect("canonical marker");
    let items_before = state.store.owner_digest_items().expect("items");
    assert_eq!(items_before.len(), 1);
    assert!(state
        .store
        .is_canonical_unavailable_failure(&items_before[0]));
    let marker_audits_before = state
        .store
        .count_audit_events_of_kind("failure.digest_unavailable")
        .expect("marker audits");
    assert_eq!(marker_audits_before, 1);

    for _ in 0..3 {
        handle_detail_command(
            &state,
            &crate::test_support::owner_surface_for(&state, 555),
            marker_id,
            1,
        )
        .await
        .expect("view canonical marker");
    }

    // Recursive-marker contract: repeated views must not create more
    // digest items or more failure.digest_unavailable audits.
    assert_eq!(
        state.store.owner_digest_items().expect("items after").len(),
        items_before.len(),
        "canonical marker re-view must not insert more digest items"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("failure.digest_unavailable")
            .expect("marker audits after"),
        marker_audits_before,
        "canonical marker re-view must not append more unavailable marker audits"
    );
    // Non-secret NULL-ref contract is preserved: the same marker remains
    // and the owner still receives the unavailable surface (3 deliveries).
    let viewed = state
        .store
        .owner_digest_item(marker_id)
        .expect("lookup")
        .expect("marker still present");
    assert!(viewed.text_ref.is_none());
    assert!(state.store.is_canonical_unavailable_failure(&viewed));
    assert_eq!(viewed.summary, "[resource] detail unavailable");
    assert_eq!(
        server.received_requests().await.expect("requests").len(),
        3,
        "repeated views must still deliver the non-secret unavailable message"
    );
}

#[tokio::test]
async fn legacy_null_ref_records_one_terminal_marker_then_stabilizes() {
    std::env::set_var("OPENSPINE_GRANT_HMAC_KEY", TEST_GRANT_KEY);
    let server = MockServer::start().await;
    mount_send_ok(&server).await;
    let state = test_state_with_telegram(telegram(&server));

    let legacy_id = state
        .store
        .insert_legacy_digest_failure("connector", "legacy summary without ref")
        .expect("legacy row");
    assert_eq!(state.store.owner_digest_items().expect("items").len(), 1);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("failure.digest_unavailable")
            .expect("marker audits"),
        0
    );

    // First legacy view may create exactly one terminal marker.
    handle_detail_command(
        &state,
        &crate::test_support::owner_surface_for(&state, 555),
        legacy_id,
        1,
    )
    .await
    .expect("first legacy view");
    let after_first = state.store.owner_digest_items().expect("items");
    assert_eq!(
        after_first.len(),
        2,
        "legacy view records one terminal marker"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("failure.digest_unavailable")
            .expect("marker audits"),
        1
    );
    let marker = after_first
        .iter()
        .find(|item| state.store.is_canonical_unavailable_failure(item))
        .expect("canonical marker present");
    assert!(marker.text_ref.is_none());

    let items_stable = state.store.owner_digest_items().expect("items").len();
    let marker_audits_stable = state
        .store
        .count_audit_events_of_kind("failure.digest_unavailable")
        .expect("marker audits");

    // Repeated legacy views and marker views must not grow items/marker audits.
    handle_detail_command(
        &state,
        &crate::test_support::owner_surface_for(&state, 555),
        legacy_id,
        1,
    )
    .await
    .expect("second legacy view");
    handle_detail_command(
        &state,
        &crate::test_support::owner_surface_for(&state, 555),
        marker.id,
        1,
    )
    .await
    .expect("marker view");
    handle_detail_command(
        &state,
        &crate::test_support::owner_surface_for(&state, 555),
        marker.id,
        1,
    )
    .await
    .expect("marker re-view");

    assert_eq!(
        state.store.owner_digest_items().expect("items").len(),
        items_stable
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("failure.digest_unavailable")
            .expect("marker audits"),
        marker_audits_stable
    );
}
