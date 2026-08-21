use super::*;
use crate::api::dispatch_tests::mint_grant_with_selection_token;
use crate::test_support::fixtures::test_state_with_telegram;
use openspine_schemas::briefcase::{BriefcaseSection, SectionKind, VisibilityClass};
use serde_json::json;

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
