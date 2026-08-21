//! Origin-symmetry proof for the external-egress disclosure hook (#207, spec
//! #204 story 7). A rated messaging-send whose (relationship, disclosure_class)
//! is uncovered must block and raise `OwnerQuestion` at the shared dispatch
//! boundary regardless of dispatch origin — worker-requested (`ActionOrigin::
//! Shell`), kernel-origin (`ActionOrigin::Kernel`), or the proactive headless
//! lane acting with no principal present. There is no second,
//! ungated path for autonomous outbound content.
//!
//! These are production-entering tests: they drive the real shared mediation
//! entrypoints (`mediate_and_dispatch_action`, `..._kernel_origin`,
//! `..._headless`), whose body runs `gate()` then the inline disclosure hook.
//! `email.send` is the catalog's real rated `DirectMessage` action (#206); the
//! kernel-origin lane additionally marks it kernel-origin in a test catalog so
//! the identical rated action can be driven down the `ActionOrigin::Kernel`
//! path. The disclosure core still resolves its egress rating from the
//! canonical catalog, so the block/OwnerQuestion outcome is the production one.

use crate::action_catalog::canonical_catalog;
use crate::api::actions::{
    mediate_and_dispatch_action, mediate_and_dispatch_action_headless,
    mediate_and_dispatch_action_kernel_origin, DispatchError, FailureSurface,
};
use crate::api::dispatch_tests::{
    insert_bound_briefcase_with_sections, mint_grant_with_selection_token_egress, OWNER_CHAT_ID,
};
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::test_state_with_telegram;
use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::briefcase::{BriefcaseSection, SectionKind, VisibilityClass};
use openspine_schemas::disclosure_policy::DisclosureClass;
use openspine_schemas::egress::EgressClass;
use openspine_schemas::identity::RelationshipKind;
use openspine_schemas::ids::PrincipalId;
use openspine_schemas::provenance::ProvenanceOrigin;
use serde_json::json;
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const BLOCK_MESSAGE: &str = "rated disclosure was blocked by kernel policy";

/// The dispatch lane whose origin symmetry is under test.
#[derive(Debug, Clone, Copy)]
enum Lane {
    /// Worker-requested dispatch (`ActionOrigin::Shell`) — the baseline #206
    /// proved for `email.send`.
    Worker,
    /// Kernel-origin dispatch (`ActionOrigin::Kernel`): the kernel acting for
    /// itself, e.g. a proactive/autonomous loop with no principal present.
    Kernel,
    /// The headless lane (`ActionOrigin::Shell`, `headless: true`) acting
    /// for the Unattended workhorse story with no principal in the loop.
    Headless,
}

/// A single Private-classified, worker-visible briefcase section. Its presence
/// makes the disclosure provenance non-empty, so the (Client, Private) scope
/// must be covered before an `email.send` may proceed. With no policy on
/// record it is uncovered — the block condition.
fn private_section() -> BriefcaseSection {
    BriefcaseSection {
        key: "private-note".to_string(),
        kind: SectionKind::Preference,
        visibility: VisibilityClass::WorkerScratch,
        depth: 0,
        disclosure_class: Some(DisclosureClass::Private),
        // Owner-sourced preference: a resolvable typed-identity origin so
        // provenance derivation succeeds and the block comes from the uncovered
        // (Client, Private) coverage stage under test — not the #225
        // unresolved-origin fail-closed path.
        origin: Some(ProvenanceOrigin::Owner {
            principal: PrincipalId::from(ulid::Ulid::new()),
        }),
        payload: json!("condition X"),
    }
}

/// Drive one uncovered `email.send` through the given dispatch lane and return
/// the mediation result plus the number of `owner.question` escalations the
/// store recorded. Each call builds fresh state (and its own Telegram mock, so
/// the mandatory OwnerQuestion delivery succeeds) to keep the audit count
/// isolated per lane.
async fn dispatch_uncovered_email_send(lane: Lane) -> (Result<(), DispatchError>, usize) {
    let server = MockServer::start().await;
    let token = "test-token";
    Mock::given(method("POST"))
        .and(path(format!("/bot{}/SendMessage", token)))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "date": 0,
                "chat": {"id": OWNER_CHAT_ID, "type": "private"},
                "text": "sent"
            }
        })))
        .mount(&server)
        .await;
    let mut state = test_state_with_telegram(TelegramConnector::with_api_url(
        token.to_string(),
        server.uri().parse().unwrap(),
    ));
    // Trust `email.send` as a kernel-origin action FOR THIS TEST so the same
    // rated action can be driven down the `ActionOrigin::Kernel` path. Starting
    // from the canonical catalog preserves `email.send`'s real `DirectMessage`
    // egress rating and counterparty-facing classification; only the
    // kernel-origin trust set is widened, and only in this test state.
    state.action_catalog = canonical_catalog()
        .with_kernel_origin([ActionId::new("owner.notify"), ActionId::new("email.send")]);
    let store = state.store.clone();
    let (grant, _) = mint_grant_with_selection_token_egress(
        &state,
        &["email.send"],
        &[EgressClass::DirectMessage],
        Timestamp::now() + Duration::from_secs(120),
    );
    // Bound Client, one Private section, NO disclosure policy → uncovered.
    insert_bound_briefcase_with_sections(
        &state,
        &grant,
        RelationshipKind::Client,
        vec![private_section()],
    );
    let surface = crate::test_support::telegram_surface(OWNER_CHAT_ID);
    let payload = json!({"body": "Please review condition X before Friday."});
    let action = ActionId::new("email.send");
    let result = match lane {
        Lane::Worker => {
            mediate_and_dispatch_action(
                &state,
                &grant,
                action,
                &surface,
                Some(&payload),
                FailureSurface::Detached,
                None,
            )
            .await
        }
        Lane::Kernel => {
            mediate_and_dispatch_action_kernel_origin(
                &state,
                &grant,
                action,
                &surface,
                Some(&payload),
                FailureSurface::Detached,
                None,
            )
            .await
        }
        Lane::Headless => {
            mediate_and_dispatch_action_headless(&state, &grant, action, &surface, Some(&payload))
                .await
        }
    };
    let owner_questions = store.count_audit_events_of_kind("owner.question").unwrap();
    (result.map(|_| ()), owner_questions)
}

/// Assert the lane's outcome is the fixed generic disclosure block with exactly
/// one routed OwnerQuestion — the same shape a worker-origin dispatch gets.
fn assert_blocked_with_owner_question(
    lane: &str,
    result: &Result<(), DispatchError>,
    questions: usize,
) {
    match result {
        Err(DispatchError::BadRequest(msg)) => assert_eq!(
            msg, BLOCK_MESSAGE,
            "{lane}: rated disclosure block must surface the fixed generic denial"
        ),
        other => panic!("{lane}: expected a disclosure block, got {other:?}"),
    }
    assert_eq!(
        questions, 1,
        "{lane}: an uncovered rated dispatch must raise exactly one OwnerQuestion"
    );
}

/// #207 acceptance criterion: a kernel-origin (`ActionOrigin::Kernel`) dispatch
/// of a rated messaging-send with an uncovered disclosure policy blocks and
/// raises `OwnerQuestion`, exactly like a worker-origin dispatch.
#[tokio::test]
async fn kernel_origin_rated_dispatch_blocks_and_raises_owner_question() {
    let (result, owner_questions) = dispatch_uncovered_email_send(Lane::Kernel).await;
    assert_blocked_with_owner_question("kernel-origin", &result, owner_questions);
}

/// #207: the proactive headless lane (Unattended workhorse, no principal
/// present) enforces disclosure identically — an uncovered rated send
/// blocks and raises `OwnerQuestion` rather than becoming a second ungated path.
#[tokio::test]
async fn proactive_headless_rated_dispatch_blocks_and_raises_owner_question() {
    let (result, owner_questions) = dispatch_uncovered_email_send(Lane::Headless).await;
    assert_blocked_with_owner_question("headless", &result, owner_questions);
}

/// #207: the disclosure block and its OwnerQuestion escalation are identical
/// across worker-, kernel-, and headless-origin dispatch of the same rated
/// action against the same uncovered policy. No origin is a second, ungated
/// path for autonomous outbound content (spec #204 story 7).
#[tokio::test]
async fn disclosure_block_is_identical_across_worker_kernel_and_headless_origins() {
    let (worker, worker_q) = dispatch_uncovered_email_send(Lane::Worker).await;
    let (kernel, kernel_q) = dispatch_uncovered_email_send(Lane::Kernel).await;
    let (headless, headless_q) = dispatch_uncovered_email_send(Lane::Headless).await;
    assert_blocked_with_owner_question("worker", &worker, worker_q);
    assert_blocked_with_owner_question("kernel", &kernel, kernel_q);
    assert_blocked_with_owner_question("headless", &headless, headless_q);
}

/// #207 criterion 2 (no ungated path): the one hand-rolled kernel-origin
/// dispatch path, `notify_owner_with_digest`, only ever dispatches
/// `owner.notify`. It is structurally safe from the disclosure gap precisely
/// because that action is unrated and owner-facing — it carries no counterparty
/// egress the disclosure hook governs. Every rated counterparty action instead
/// flows through the shared mediate-and-dispatch entrypoint, where the hook is
/// origin-symmetric (proven above). If `owner.notify` ever gained an egress
/// rating this invariant would fail, flagging that the hand-rolled path must be
/// folded into the shared entrypoint before it could carry rated content.
#[test]
fn owner_notify_is_unrated_and_owner_facing_so_the_notify_path_carries_no_gated_egress() {
    let catalog = canonical_catalog();
    let owner_notify = ActionId::new("owner.notify");
    assert!(
        catalog.is_kernel_origin(&owner_notify),
        "owner.notify is the hand-rolled kernel-origin path's action"
    );
    assert_eq!(
        catalog.egress_class_for(&owner_notify),
        None,
        "owner.notify must stay unrated: the hand-rolled kernel path cannot construct \
         the counterparty/briefcase disclosure context a rated action requires"
    );
    assert!(
        !catalog.is_counterparty_facing(&owner_notify),
        "owner.notify is owner-facing, never a counterparty egress"
    );
}
