//! What a decision actually commits, per surface: the terminal originating its
//! own decision, receipt truthfulness, the typed refusals, and the kernel's
//! adapter-independent narrowing guard. Split from
//! `owner_review_adapter_bounds_tests.rs` for the 500-line gate.

use jiff::Timestamp;
use openspine_schemas::owner_review::{OwnerReviewRequest, OwnerReviewState};
use ulid::Ulid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::owner_review_surface::persist_owner_review;
use super::owner_review_tests::{
    owner_review, owner_review_offering, owner_review_with_target_count,
};
use super::{handle_owner_update, handle_terminal_message, AppState};
use crate::telegram::{CallbackQueryUpdate, TelegramConnector};
use crate::test_support::fixtures::{owner_update, test_state, test_state_with_telegram};

fn terminal_state() -> (AppState, tokio::sync::mpsc::UnboundedReceiver<String>) {
    let mut state = test_state();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    state.terminal_reply_tx = Some(tx);
    (state, rx)
}

fn persist(state: &AppState, review: &OwnerReviewRequest) {
    let now = Timestamp::now();
    persist_owner_review(
        state,
        review,
        state.owner.principal_id.as_ulid(),
        now + std::time::Duration::from_secs(3600),
        now,
        None,
    )
    .unwrap();
}

fn token(review: &OwnerReviewRequest) -> String {
    review.binding_digest().as_str()["sha256:".len()..][..12].to_string()
}

/// `Edit` is a legal member of `OwnerReviewDecision`, so it passes the
/// membership check when the stored review offers it — and is then refused,
/// because the kernel cannot synthesise the replacement review an edit needs.
/// This pins the refusal as typed and state-preserving rather than a silent
/// no-op or a partial mutation.
#[tokio::test]
async fn edit_is_refused_as_requiring_a_replacement_review() {
    let (state, mut rx) = terminal_state();
    let review = owner_review_offering(
        Ulid::new(),
        state.owner.principal_id.as_ulid(),
        "Edit is offered here".into(),
        &[openspine_schemas::owner_review::OwnerReviewDecision::Edit],
    );
    persist(&state, &review);

    handle_terminal_message(
        &state,
        format!("/review {} {} edit", review.id, token(&review)),
    )
    .await
    .unwrap();

    let refusal = rx.recv().await.expect("edit refusal");
    assert!(
        refusal.contains("Review decision refused"),
        "edit must be refused, got: {refusal}"
    );
    assert_eq!(
        state
            .store
            .owner_review_row(review.id)
            .unwrap()
            .unwrap()
            .state,
        OwnerReviewState::Pending,
        "a refused edit leaves the review untouched"
    );
    assert_eq!(
        state.store.count_owner_reviews().unwrap(),
        1,
        "a refused edit creates no replacement review"
    );
}

/// A callback digest token that is not a prefix of the stored binding digest
/// is refused before any state transition — the short token is an input
/// encoding, never a weaker binding.
#[tokio::test]
async fn a_digest_token_that_does_not_prefix_the_stored_digest_is_refused() {
    let (state, mut rx) = terminal_state();
    let review = owner_review(
        Ulid::new(),
        state.owner.principal_id.as_ulid(),
        "Token must prefix the digest".into(),
    );
    persist(&state, &review);

    let mut wrong = token(&review);
    wrong.replace_range(0..1, if wrong.starts_with('a') { "b" } else { "a" });
    handle_terminal_message(&state, format!("/review {} {wrong} approve", review.id))
        .await
        .unwrap();

    let refusal = rx.recv().await.expect("token refusal");
    assert!(refusal.contains("Review decision refused"));
    assert_eq!(
        state
            .store
            .owner_review_row(review.id)
            .unwrap()
            .unwrap()
            .state,
        OwnerReviewState::Pending
    );
    assert_eq!(
        state.store.owner_review_decision_digest(review.id).unwrap(),
        None,
        "a mismatched token records no decision"
    );
}

/// A review receipt is truthful about what the decision committed: it names
/// the lifecycle outcome and the binding digest, and it never claims that an
/// external effect ran. Approving a review activates the derived standing
/// rule; the effect is a separate, separately gated request (D-007), so a
/// receipt that spoke of delivery would be claiming something the review path
/// did not do.
#[tokio::test]
async fn an_approval_receipt_reports_the_lifecycle_outcome_and_no_effect() {
    let (state, mut rx) = terminal_state();
    let review = owner_review(
        Ulid::new(),
        state.owner.principal_id.as_ulid(),
        "Approve commits authority, not an effect".into(),
    );
    persist(&state, &review);

    handle_terminal_message(
        &state,
        format!("/review {} {} approve", review.id, token(&review)),
    )
    .await
    .unwrap();

    let receipt = rx.recv().await.expect("approval receipt");
    assert!(receipt.contains(review.binding_digest().as_str()));
    assert!(
        receipt.contains("activated standing rule"),
        "the receipt names the committed lifecycle outcome, got: {receipt}"
    );
    for claim in ["sent", "delivered", "draft created", "executed"] {
        let unclaimed = !receipt.to_lowercase().contains(claim);
        assert!(
            unclaimed,
            "a review receipt must not claim an external effect ({claim}): {receipt}"
        );
    }

    // Replay reports unchanged rather than a second committed outcome.
    handle_terminal_message(
        &state,
        format!("/review {} {} approve", review.id, token(&review)),
    )
    .await
    .unwrap();
    let replay = rx.recv().await.expect("replay receipt");
    assert!(replay.contains("replay: unchanged"), "got: {replay}");
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("owner_review.decision")
            .unwrap(),
        1,
        "a replayed approval leaves exactly one durable decision"
    );
}

/// **A surface cannot add scope**, and this is the guard that makes the claim
/// hold for *every* surface rather than for the one whose delta parser happens
/// to be strict. It calls `OwnerReviewRequest::narrowed_review` directly with
/// a genuinely widened `ReviewedActionScope`, bypassing the terminal parser
/// entirely, so a future Telegram or web narrow surface inherits the refusal.
#[test]
fn kernel_refuses_a_widened_narrow_regardless_of_the_delta_parser() {
    use openspine_schemas::owner_review::OwnerReviewNarrowError;
    use openspine_schemas::reviewed_scope::ReviewedActionScope;

    let state = test_state();
    // A review bound to ONE target, and a well-formed scope over TWO.
    let review = owner_review_with_target_count(
        Ulid::new(),
        state.owner.principal_id.as_ulid(),
        "One reviewed target".into(),
        1,
    );
    let widened = ReviewedActionScope::derive(
        &super::owner_review_tests::email_context_with_target_count(2),
    )
    .unwrap();
    assert!(
        widened.binding_is_valid(),
        "the widened scope is well-formed, so the refusal is about scope and not validity"
    );

    let refused = review.narrowed_review(Ulid::new(), widened);
    assert!(
        matches!(refused, Err(OwnerReviewNarrowError::NotNarrower)),
        "widening MUST be refused as NotNarrower, got {refused:?}"
    );
}

/// The terminal ORIGINATES a decision on its own review, and Telegram then
/// reaches the very same stored row and binding digest.
///
/// `telegram_and_terminal_decide_the_same_digest_bound_review_row` drives
/// Telegram first and the terminal second, so its terminal leg is a replay —
/// it proves idempotency, not terminal-originated decision power. This is the
/// mirror image: the terminal is the surface that actually moves the review,
/// and the Telegram callback that follows is the one that finds the decision
/// already made, against the identical digest.
#[tokio::test]
async fn terminal_originates_a_decision_and_telegram_reaches_the_same_row() {
    let telegram = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/AnswerCallbackQuery"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true, "result": true
        })))
        .mount(&telegram)
        .await;
    Mock::given(method("POST"))
        .and(path("/bottest-token/SendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "result": {
                "message_id": 1, "date": 1,
                "chat": {"id": 555, "type": "private"}, "text": "ok"
            }
        })))
        .mount(&telegram)
        .await;
    let connector =
        TelegramConnector::with_api_url("test-token".into(), telegram.uri().parse().unwrap());
    let mut state = test_state_with_telegram(connector);
    let (reply_tx, mut reply_rx) = tokio::sync::mpsc::unbounded_channel();
    state.terminal_reply_tx = Some(reply_tx);

    let review = owner_review(
        Ulid::new(),
        state.owner.principal_id.as_ulid(),
        "The terminal decides this one first.".into(),
    );
    persist(&state, &review);

    // Terminal originates the decision.
    handle_terminal_message(
        &state,
        format!("/review {} {} approve", review.id, token(&review)),
    )
    .await
    .unwrap();
    let terminal_receipt = reply_rx.recv().await.expect("terminal receipt");
    let originated = !terminal_receipt.contains("replay: unchanged");
    assert!(
        originated,
        "the terminal must be the originating surface here, got: {terminal_receipt}"
    );
    assert!(terminal_receipt.contains(review.binding_digest().as_str()));

    let row = state.store.owner_review_row(review.id).unwrap().unwrap();
    assert_eq!(row.state, OwnerReviewState::Approved);
    assert_eq!(
        state.store.owner_review_decision_digest(review.id).unwrap(),
        Some(review.binding_digest().clone()),
        "the terminal's decision is bound to the digest it was shown"
    );
    let (rule_id, rule_version) = state
        .store
        .owner_review_standing_rule(review.id)
        .unwrap()
        .expect("a terminal approval activates and binds the derived rule");
    assert!(state
        .store
        .standing_rule_is_current(&rule_id, rule_version)
        .unwrap());

    // Telegram now reaches the same stored object, against the same digest.
    let mut update = owner_update("");
    update.text = None;
    update.callback_query = Some(CallbackQueryUpdate {
        id: "owner-review-approve".into(),
        data: Some(format!("or:a:{}:{}", review.id, token(&review))),
    });
    handle_owner_update(&state, &update).await.unwrap();

    let after = state.store.owner_review_row(review.id).unwrap().unwrap();
    assert_eq!(after.id, row.id, "both surfaces address one review row");
    assert_eq!(after.artifact_ref, row.artifact_ref);
    assert_eq!(after.state, OwnerReviewState::Approved);
    assert_eq!(
        state.store.owner_review_decision_digest(review.id).unwrap(),
        Some(review.binding_digest().clone()),
        "the Telegram leg lands on the identical binding digest"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("owner_review.decision")
            .unwrap(),
        1,
        "the second surface records no second decision"
    );
}
