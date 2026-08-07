//! Write-admission ordering: the executor takes its connector write permit
//! BEFORE recording the pending-write fence (#127), so a refused admission
//! leaves no fence row at all. Split from `approval_draft_reconcile_tests.rs`
//! to keep both files under the 500-line gate.

use super::approval_draft_reconcile_tests::total_pending_draft_write_rows;
use super::{
    approval_fixture_grant, approval_fixture_request, gmail_with_token_mock, thread_with_sender,
};
use crate::api::effect_executors::EffectOutcome;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn rate_limited_write_admission_is_refused_without_a_fence_row() {
    // #127 ordering invariant: the executor takes the write's connector permit
    // BEFORE recording the pending-write fence, so a rejected write admission
    // — which has polled no WRITE future and attempted no provider write — is a
    // true pre-effect refusal that leaves NO fence row at all. An
    // inserted-then-resolved row would falsely record an attempted Gmail write
    // in the reconciliation table.
    //
    // This is the only deterministic pin on that ordering:
    // `unavailable_gmail_connector_refuses_before_any_fence_row` says in its
    // own comment that it cannot pin it, because it blocks at the fetch and
    // never reaches the write admission at all.
    //
    // Setup (openspine#163): one permit, refilled only after an hour. The
    // executor's live thread fetch takes the single permit and the subsequent
    // write admission is refused — with no dependence on how long the fetch
    // took. The previous shape drained nine of the default ten permits and
    // relied on the tenth not being refilled within 100ms, which a contended
    // machine routinely broke: the fetch overran the window, the bucket
    // refilled, the write was admitted, and the assertion failed with
    // `FailedAfterAttempt`. The refusal is now a property of the config, not
    // of the elapsed time.
    let token_server = MockServer::start().await;
    let api_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/thread-1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(thread_with_sender("alice@example.com")),
        )
        .mount(&api_server)
        .await;
    // No drafts mock: the rejection must happen before any write is attempted.
    let gmail = gmail_with_token_mock(&token_server, &api_server).await;
    let state = crate::test_support::fixtures::test_state_with_gmail_and_rate_limit(
        gmail,
        crate::connector_reality::RateLimitConfig {
            capacity: 1,
            refill_after: std::time::Duration::from_secs(3600),
        },
    );
    let grant = approval_fixture_grant();
    let request = approval_fixture_request(
        &state,
        grant.id,
        "Re: invoice",
        "sounds good",
        "alice@example.com",
    );

    let outcome = crate::pipeline::approval::create_approved_draft(&state, &grant, &request, 555)
        .await
        .unwrap();

    assert_eq!(
        outcome,
        EffectOutcome::RefusedPreEffect,
        "a rejected write admission never attempted a provider write"
    );
    // The refusal must be at the WRITE admission, not earlier. Proving the
    // read-only thread fetch happened is what makes this a pin on
    // permit-before-fence rather than on any generic pre-effect refusal: the
    // executor got as far as re-deriving its target and was then refused at
    // the write permit, with the fence still unwritten.
    let thread_fetches = api_server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|req| req.url.path() == "/gmail/v1/users/me/threads/thread-1")
        .count();
    assert_eq!(
        thread_fetches, 1,
        "the executor completed its read-only re-derivation before the write admission"
    );
    let draft_writes = api_server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|req| req.url.path() == "/gmail/v1/users/me/drafts")
        .count();
    assert_eq!(draft_writes, 0, "no provider write was attempted");
    assert_eq!(
        total_pending_draft_write_rows(&state),
        0,
        "the fence must be recorded only after the write permit is held"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.creation_failed")
            .unwrap(),
        1
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("draft.created")
            .unwrap(),
        0
    );
}
