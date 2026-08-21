//! Origin-closure (D-174 / spec #220) dispatch-integration regression tests.
//!
//! These exercise the closure at `enforce_disclosure_egress` — the single
//! connector-agnostic chokepoint EVERY dispatch origin funnels through
//! (worker-requested via `mediate_and_dispatch_action`, kernel-origin/proactive
//! via `..._kernel_origin`). The origin-symmetry of that chokepoint — that both
//! origins reach it with no second ungated path — is proven in
//! `api/disclosure_origin_tests`; the origin-closure stage lives inside the same
//! function, so it is enforced identically regardless of dispatch origin.
//!
//! The stage runs ONLY after disclosure coverage passes, so each case seeds a
//! covering (Client, Internal, Search) policy first, then varies origin ×
//! recipient × grant caveat. The closure is dormant on the v1 single-owner
//! owner grant (which carries no `ProvenanceLabelAllowlist` caveat, so every
//! origin is authorized) and strict once a grant carries a narrowing caveat —
//! exactly the shape a counterparty-scoped worker sub-grant adopts (user
//! story 10). The worker-facing fixed generic denial is covered by
//! `disclosure_regression_tests::worker_denial_has_no_debug_leak`; here we
//! assert the kernel-side outcome plus the reconstructible audit row.

use super::*;
use crate::api::dispatch_tests::{
    insert_bound_briefcase_with_sections, mint_grant_with_selection_token,
};
use crate::test_support::fixtures::test_state_with_telegram;

/// `insert_bound_briefcase_with_sections` binds this counterparty id as the
/// recipient, so the origin closure compares each item's origin against it.
const RECIPIENT_ID: u128 = 11;

fn state() -> crate::pipeline::AppState {
    test_state_with_telegram(crate::telegram::TelegramConnector::new(
        "bottest-token".to_string(),
    ))
}

fn counterparty(id: u128) -> ProvenanceOrigin {
    ProvenanceOrigin::Counterparty {
        identity: IdentityRef::from(Ulid::from(id)),
    }
}

fn internal_item(origin: ProvenanceOrigin) -> ClassifiedBriefcaseItem {
    ClassifiedBriefcaseItem {
        item_ref: openspine_schemas::artifact::ArtifactRef {
            digest: openspine_schemas::digest::digest_of_bytes(b"payload"),
            schema_version: 1,
        },
        disclosure_class: DisclosureClass::Internal,
        origin: Some(origin),
    }
}

fn request(origin: ProvenanceOrigin) -> DisclosureRequest {
    DisclosureRequest {
        raw_query: "reply body".to_string(),
        sensitive_terms: BTreeSet::new(),
        action_id: ActionId::new("web.search"),
        relationship: RelationshipKind::Client,
        provenance: DisclosureProvenance {
            items: vec![internal_item(origin)],
        },
    }
}

/// Append the D-174 narrowing caveat a counterparty-scoped sub-grant carries.
/// `enforce_disclosure_egress` reads the caveat chain (the gate verified the MAC
/// upstream), so appending to the sealed tip is faithful for this seam test.
fn narrow_to(grant: &mut TaskGrant, origins: Vec<ProvenanceOrigin>) {
    grant
        .chain
        .last_mut()
        .unwrap()
        .added_caveats
        .push(openspine_schemas::grant_chain::Caveat::ProvenanceLabelAllowlist { origins });
}

/// Seed a covering (Client, Internal, Search) policy and a grant whose briefcase
/// is bound to the recipient counterparty, so only the origin closure varies.
async fn seed(state: &crate::pipeline::AppState) -> TaskGrant {
    let now = Timestamp::now();
    record_owner_answer(
        &state.store,
        DisclosurePolicyKey {
            relationship: RelationshipKind::Client,
            disclosure_class: DisclosureClass::Internal,
        },
        EgressClass::Search,
        vec![],
        now,
    )
    .unwrap();
    let (grant, _) = mint_grant_with_selection_token(
        state,
        &["web.search"],
        now + std::time::Duration::from_secs(120),
    );
    insert_bound_briefcase_with_sections(state, &grant, RelationshipKind::Client, vec![]);
    grant
}

/// A counterparty-X datum is blocked from egress to the bound recipient
/// counterparty Y once the grant carries the narrowing provenance caveat, even
/// though (Client, Internal, Search) coverage passes. The block is
/// reconstructible from the typed origin + sensitivity + recipient + egress
/// class recorded in the `disclosure.cross_identity_blocked` audit row (Auditor
/// story). Closes Bell's cross-counterparty leak.
#[tokio::test]
async fn counterparty_origin_blocked_from_a_different_recipient() {
    let state = state();
    let mut grant = seed(&state).await;
    narrow_to(&mut grant, vec![]);
    let result = enforce_disclosure_egress(&state, &grant, request(counterparty(22))).await;
    assert!(result.is_err(), "cross-counterparty egress must be blocked");
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("disclosure.cross_identity_blocked")
            .unwrap(),
        1,
        "the block must record its reconstructible cross-identity audit row"
    );
}

/// Bell "internal data to a stranger": owner-origin, non-public data cannot
/// reach a counterparty recipient under a narrowing grant (user story 3).
#[tokio::test]
async fn owner_origin_blocked_from_a_counterparty_recipient() {
    let state = state();
    let mut grant = seed(&state).await;
    narrow_to(&mut grant, vec![]);
    let owner = ProvenanceOrigin::Owner {
        principal: openspine_schemas::ids::PrincipalId::from(Ulid::new()),
    };
    let result = enforce_disclosure_egress(&state, &grant, request(owner)).await;
    assert!(
        result.is_err(),
        "owner-internal data to a stranger must block"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("disclosure.cross_identity_blocked")
            .unwrap(),
        1
    );
}

/// A counterparty's own datum reaches that same bound counterparty recipient.
#[tokio::test]
async fn counterparty_origin_reaches_its_own_recipient() {
    let state = state();
    let mut grant = seed(&state).await;
    narrow_to(&mut grant, vec![]);
    let result =
        enforce_disclosure_egress(&state, &grant, request(counterparty(RECIPIENT_ID))).await;
    assert!(result.is_ok(), "same-identity egress must be allowed");
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("disclosure.cross_identity_blocked")
            .unwrap(),
        0
    );
}

/// The #226 widening path: a `ProvenanceLabelAllowlist` caveat that names
/// counterparty X lets X's datum egress to a different recipient.
#[tokio::test]
async fn an_authorizing_caveat_lets_a_named_origin_egress() {
    let state = state();
    let mut grant = seed(&state).await;
    narrow_to(&mut grant, vec![counterparty(22)]);
    let result = enforce_disclosure_egress(&state, &grant, request(counterparty(22))).await;
    assert!(result.is_ok(), "an authorized origin must egress");
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("disclosure.cross_identity_blocked")
            .unwrap(),
        0
    );
}

/// The v1 single-owner owner grant carries no narrowing caveat, so every origin
/// is authorized and the closure is dormant — a cross-identity send is NOT
/// over-blocked, preserving Lyra's owner-directed flows.
#[tokio::test]
async fn owner_grant_without_a_caveat_does_not_over_block() {
    let state = state();
    let grant = seed(&state).await;
    let result = enforce_disclosure_egress(&state, &grant, request(counterparty(22))).await;
    assert!(
        result.is_ok(),
        "the un-narrowed owner grant authorizes every origin"
    );
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("disclosure.cross_identity_blocked")
            .unwrap(),
        0
    );
}
