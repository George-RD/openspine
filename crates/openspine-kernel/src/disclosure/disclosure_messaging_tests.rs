use super::*;
use crate::api::dispatch_tests::{
    insert_bound_briefcase_with_sections, mint_grant_with_selection_token,
    mint_grant_with_selection_token_egress,
};
use crate::api::tests::{post_action, start_server};
use crate::telegram::TelegramConnector;
use crate::test_support::fixtures::test_state_with_telegram;
use openspine_schemas::briefcase::{BriefcaseSection, SectionKind, VisibilityClass};
use serde_json::{json, Value};
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// #205 prefactor: the disclosure core is preparation-agnostic. A request
/// whose provenance was derived by the non-query messaging preparation shape
/// (empty `sensitive_terms`, the composed body left un-generalized) drives the
/// same Block→OwnerQuestion / Allow-after-owner-answer core outcome as
/// web-search preparation. Uses an existing rated action (`web.search`) because
/// no catalog action is rated for messaging egress yet in this prefactor.
#[tokio::test]
async fn messaging_preparation_drives_core_without_query_generalization() {
    let state = test_state_with_telegram(crate::telegram::TelegramConnector::new(
        "bottest-token".to_string(),
    ));
    let (grant, _) = mint_grant_with_selection_token(
        &state,
        &["web.search"],
        Timestamp::now() + std::time::Duration::from_secs(120),
    );
    let sections = vec![BriefcaseSection {
        key: "private-note".to_string(),
        kind: SectionKind::Preference,
        visibility: VisibilityClass::WorkerScratch,
        depth: 0,
        disclosure_class: Some(DisclosureClass::Private),
        origin: None,
        payload: json!("condition X"),
    }];
    // Non-query preparation: the message body carries the sensitive term
    // verbatim (recipients read it) and no term redaction is applied.
    let messaging_request = || {
        prepare_messaging_disclosure(
            ActionId::new("web.search"),
            RelationshipKind::Client,
            "recipient-selection-token",
            "Please review condition X before Friday.".to_string(),
            &sections,
        )
        .expect("messaging preparation derives provenance from classified sections")
    };
    let request = messaging_request();
    assert!(
        request.sensitive_terms.is_empty(),
        "messaging preparation never redacts the body"
    );
    assert_eq!(
        request.raw_query,
        "Please review condition X before Friday."
    );
    assert!(request
        .provenance
        .classes()
        .contains(&DisclosureClass::Private));

    // Uncovered (Client, Private) must not allow. Reservation cancellation
    // runs before escalation delivery, so the Err is Blocked when the test
    // connector delivers and Store when it is unreachable (D-058); either way
    // the core refused to let messaging-derived provenance through.
    assert!(
        enforce_disclosure_egress(&state, &grant, request)
            .await
            .is_err(),
        "uncovered messaging provenance must block"
    );

    // Once the owner answers for that exact scope, the same messaging-derived
    // provenance is allowed through.
    record_owner_answer(
        &state.store,
        DisclosurePolicyKey {
            relationship: RelationshipKind::Client,
            disclosure_class: DisclosureClass::Private,
        },
        EgressClass::Search,
        vec![],
        Timestamp::now(),
    )
    .unwrap();
    assert!(
        enforce_disclosure_egress(&state, &grant, messaging_request())
            .await
            .is_ok()
    );
}

/// A single Private-classified, worker-visible briefcase section. Its presence
/// makes the disclosure provenance non-empty so the (Client, Private) scope
/// must be covered before an `email.send` may proceed.
fn private_section() -> BriefcaseSection {
    BriefcaseSection {
        key: "private-note".to_string(),
        kind: SectionKind::Preference,
        visibility: VisibilityClass::WorkerScratch,
        depth: 0,
        disclosure_class: Some(DisclosureClass::Private),
        payload: json!("condition X"),
    }
}

/// #206 acceptance criterion 3: an `email.send` whose (relationship,
/// disclosure_class) has NO disclosure policy on record blocks at the dispatch
/// boundary and routes an `OwnerQuestion` — the same block shape web.search
/// gets, driven through the real router. The worker sees only the fixed generic
/// denial (no relationship/class/question debug leak).
#[tokio::test]
async fn email_send_uncovered_disclosure_blocks_through_dispatch_and_routes_owner_question() {
    let server = MockServer::start().await;
    let token = "test-token";
    Mock::given(method("POST"))
        .and(path(format!("/bot{}/SendMessage", token)))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "date": 0,
                "chat": {"id": 555, "type": "private"},
                "text": "sent"
            }
        })))
        .mount(&server)
        .await;
    let state = test_state_with_telegram(TelegramConnector::with_api_url(
        token.to_string(),
        server.uri().parse().unwrap(),
    ));
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
    let (addr, handle) = start_server(state).await;

    let resp = post_action(
        addr,
        &grant.task_token,
        "email.send",
        Some(json!({"body": "Please review condition X before Friday."})),
    )
    .await;
    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(
        body,
        json!({"error": "rated disclosure was blocked by kernel policy"})
    );
    let leaked = body.to_string();
    for needle in [
        "Client",
        "Private",
        "condition X",
        "/disclosure",
        "disclosure_class",
        "relationship",
    ] {
        assert!(
            !leaked.contains(needle),
            "worker denial must not leak {needle}: {leaked}"
        );
    }
    // The OwnerQuestion escalation was routed to the owner surface.
    assert_eq!(
        store.count_audit_events_of_kind("owner.question").unwrap(),
        1
    );
    handle.abort();
}

/// #206 acceptance criterion 4 (cancel half), through the real router: an
/// `email.send` whose scope IS owner-approved passes the disclosure gate and
/// reserves the D-107 envelope, then fails closed with `NoExecutor` (email.send
/// has no executor, by design — mirrors web.search). The reservation is
/// cancelled (NotAttempted) and the envelope budget is fully restored.
#[tokio::test]
async fn email_send_covered_disclosure_reaches_no_executor_and_cancels_reservation() {
    let state = test_state_with_telegram(TelegramConnector::new("bottest-token".to_string()));
    let store = state.store.clone();
    let now = Timestamp::now();
    let key = DisclosurePolicyKey {
        relationship: RelationshipKind::Client,
        disclosure_class: DisclosureClass::Private,
    };
    record_owner_answer(&store, key, EgressClass::DirectMessage, vec![], now).unwrap();
    let (grant, _) = mint_grant_with_selection_token_egress(
        &state,
        &["email.send"],
        &[EgressClass::DirectMessage],
        now + Duration::from_secs(120),
    );
    insert_bound_briefcase_with_sections(
        &state,
        &grant,
        RelationshipKind::Client,
        vec![private_section()],
    );
    let (addr, handle) = start_server(state).await;

    let resp = post_action(
        addr,
        &grant.task_token,
        "email.send",
        Some(json!({"body": "Please review condition X before Friday."})),
    )
    .await;
    // Disclosure passed; dispatch then fails closed with NoExecutor -> 500.
    assert_eq!(resp.status(), 500);
    assert_eq!(
        resp.json::<Value>().await.unwrap()["error"],
        "internal_error"
    );
    // The cancelled reservation leaked no budget: the envelope's full rate
    // window is reservable again (mirrors the web.search block regression).
    let action = action_for_scope(key, EgressClass::DirectMessage);
    for _ in 0..5 {
        let (rule, reservation) = store
            .consult_and_reserve_standing_rule(&action, now)
            .unwrap()
            .expect("envelope visible");
        let reservation = reservation.expect("cancelled dispatch must not leak budget");
        assert!(store
            .finalize_standing_rule_reservation(&rule.rule_id, rule.version, &reservation, now)
            .unwrap());
    }
    handle.abort();
}

/// #206 acceptance criterion 4 (finalize half): proven at the
/// `enforce_disclosure_egress` seam by design (coordinator ruling
/// msg_d5358419bf8a). email.send keeps web.search's structural no-executor
/// guarantee, so finalize-on-success is not reachable through prod dispatch;
/// the reservation's finalize arm is proven here, where the effect would have
/// succeeded. An owner-approved (Client, Private) scope lets messaging-derived
/// provenance through and the reserved envelope finalizes as committed usage.
#[tokio::test]
async fn email_send_covered_messaging_disclosure_finalizes_reservation_at_seam() {
    let state = test_state_with_telegram(TelegramConnector::new("bottest-token".to_string()));
    let store = state.store.clone();
    let now = Timestamp::now();
    let key = DisclosurePolicyKey {
        relationship: RelationshipKind::Client,
        disclosure_class: DisclosureClass::Private,
    };
    record_owner_answer(&store, key, EgressClass::DirectMessage, vec![], now).unwrap();
    let (grant, _) =
        mint_grant_with_selection_token(&state, &["email.send"], now + Duration::from_secs(120));
    let sections = vec![private_section()];
    let request = prepare_messaging_disclosure(
        ActionId::new("email.send"),
        RelationshipKind::Client,
        "recipient-selection-token",
        "Please review condition X before Friday.".to_string(),
        &sections,
    )
    .expect("messaging preparation derives provenance from classified sections");
    assert!(
        request.sensitive_terms.is_empty(),
        "messaging preparation never redacts the body"
    );
    let enforced = enforce_disclosure_egress(&state, &grant, request)
        .await
        .expect("owner-approved messaging disclosure allows");
    assert!(
        !enforced.reservations.is_empty(),
        "a covered disclosure reserves the D-107 envelope"
    );
    // Finalize on success (mirrors settle_reservations' ConfirmedSuccess arm):
    // each reserved envelope commits its usage.
    for (rule_id, version, reservation_id) in &enforced.reservations {
        assert!(
            store
                .finalize_standing_rule_reservation(rule_id, *version, reservation_id, now)
                .unwrap(),
            "a live reservation must finalize as committed usage"
        );
    }
}
