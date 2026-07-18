//! Skill store hardening tests (AD-040/AD-041): fail-closed schema version,
//! monotonic install/update guards, and atomic version retirement.

use jiff::Timestamp;
use openspine_schemas::digest::Digest;
use openspine_schemas::skill::{Skill, SkillProvenance, SkillState, SkillVisibility};
use ulid::Ulid;

use crate::skill::ceremony::install_skill;
use crate::skill::ceremony::CeremonyToken;
use crate::skill::review::run_promotion_review;
use crate::store::skill_read_queries::installed_skills_for_agent_and_pack;
use crate::store::skill_store::{
    count_skill_rows_for_test, get_skill, insert_skill, promote_skill,
};
use crate::store::Store;

fn skill_with(id: &str, provenance: SkillProvenance, version: u32, agent: &str) -> Skill {
    let visibility = SkillVisibility {
        agents: vec![agent.to_string()],
        packs: vec![],
    };
    let body = format!("competence text for {id} v{version}");
    let digest = Skill::digest_of_body(&body);
    Skill {
        id: id.to_string(),
        schema_version: 1,
        version,
        provenance,
        state: SkillState::PendingReview,
        title: format!("skill {id}"),
        body,
        task_shape: vec!["email_reply".to_string()],
        visibility,
        content_digest: digest,
    }
}

/// Record a preview bound to `owner_principal` so `promote_skill`
/// (which consumes the digest+principal-bound preview atomically) can land.
#[allow(clippy::too_many_arguments)]
fn record_preview(
    store: &Store,
    skill_id: &str,
    version: u32,
    owner_principal: &str,
    digest: &Digest,
    provenance_summary: &str,
    prior_diff: &str,
    current_diff: &str,
) {
    let conn = store.conn.lock();
    crate::store::skill_preview_records::record_skill_preview(
        &conn,
        skill_id,
        version,
        owner_principal,
        digest,
        provenance_summary,
        prior_diff,
        current_diff,
        "rendered preview summary",
    )
    .unwrap();
}

#[test]
fn unsupported_schema_version_is_rejected_fail_closed() {
    let store = Store::open_in_memory().unwrap();
    let mut skill = skill_with("v2_schema", SkillProvenance::ShippedSeed, 1, "agent_a");
    skill.schema_version = 2;
    let err = insert_skill(
        &store,
        &skill,
        Timestamp::now(),
        &CeremonyToken::test_token(),
    )
    .unwrap_err();
    assert!(matches!(
        err,
        crate::store::StoreError::UnsupportedSkillSchemaVersion(2)
    ));
    // Nothing was written.
    assert_eq!(count_skill_rows_for_test(&store).unwrap(), 0);
    // Nothing was written.
}

#[test]
fn oversized_skill_id_is_rejected_before_preview_rendering() {
    let store = Store::open_in_memory().unwrap();
    let skill = skill_with(&"x".repeat(129), SkillProvenance::ShippedSeed, 1, "agent_a");
    let err = insert_skill(
        &store,
        &skill,
        Timestamp::now(),
        &CeremonyToken::test_token(),
    )
    .unwrap_err();
    assert!(matches!(err, crate::store::StoreError::SkillLifecycle(_)));
    assert_eq!(count_skill_rows_for_test(&store).unwrap(), 0);
}

#[test]
fn trusted_install_refuses_lower_version_when_higher_exists_any_state() {
    let store = Store::open_in_memory().unwrap();
    // Install v2 first (as a mined skill still pending review — a higher
    // version in a non-Installed state).
    let mut hi = skill_with("shared", SkillProvenance::MinerDistilled, 2, "agent_a");
    install_skill(&store, &mut hi, Timestamp::now()).unwrap();
    assert_eq!(
        get_skill(&store, "shared", 2).unwrap().unwrap().state,
        SkillState::PendingReview
    );

    // Installing v1 trusted must be refused (would be a downgrade / revive).
    let lo = skill_with("shared", SkillProvenance::ShippedSeed, 1, "agent_a");
    let err =
        insert_skill(&store, &lo, Timestamp::now(), &CeremonyToken::test_token()).unwrap_err();
    assert!(matches!(err, crate::store::StoreError::SkillLifecycle(_)));
    assert!(get_skill(&store, "shared", 1).unwrap().is_none());
}

#[test]
fn trusted_install_refuses_equal_version_reinstall() {
    let store = Store::open_in_memory().unwrap();
    let v1 = skill_with("dup", SkillProvenance::ShippedSeed, 1, "agent_a");
    insert_skill(&store, &v1, Timestamp::now(), &CeremonyToken::test_token()).unwrap();

    let v1b = skill_with("dup", SkillProvenance::ShippedSeed, 1, "agent_a");
    let err =
        insert_skill(&store, &v1b, Timestamp::now(), &CeremonyToken::test_token()).unwrap_err();
    assert!(matches!(err, crate::store::StoreError::SkillLifecycle(_)));
}

#[test]
fn promotion_retires_lower_installed_version_atomically() {
    let store = Store::open_in_memory().unwrap();
    // v1 trusted -> Installed.
    let v1 = skill_with("retire", SkillProvenance::ShippedSeed, 1, "agent_a");
    insert_skill(&store, &v1, Timestamp::now(), &CeremonyToken::test_token()).unwrap();

    // v2 mined -> PendingReview, then promoted through the AD-110 evaluator.
    let mut v2 = skill_with("retire", SkillProvenance::MinerDistilled, 2, "agent_a");
    install_skill(&store, &mut v2, Timestamp::now()).unwrap();
    let reviewed = run_promotion_review(&store, &get_skill(&store, "retire", 2).unwrap().unwrap())
        .expect("benign mined skill should pass review");
    // Owner previews v2 (bound to this principal) before promoting.
    let principal = Ulid::new();
    record_preview(
        &store,
        "retire",
        2,
        &principal.to_string(),
        &v2.content_digest,
        "MinerDistilled",
        "",
        "digest",
    );
    let promoted =
        promote_skill(&store, &reviewed, principal, &CeremonyToken::test_token()).unwrap();
    assert_eq!(promoted.version, 2);
    assert_eq!(promoted.state, SkillState::Installed);

    // v1 must now be Retired, and the agent must see only v2 (highest active).
    assert_eq!(
        get_skill(&store, "retire", 1).unwrap().unwrap().state,
        SkillState::Retired
    );
    let selected = installed_skills_for_agent_and_pack(&store, "agent_a", "").unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].version, 2);
}

#[test]
fn promotion_refuses_when_higher_installed_exists() {
    let store = Store::open_in_memory().unwrap();
    // v2 trusted -> Installed.
    let v2 = skill_with("bump", SkillProvenance::ShippedSeed, 2, "agent_a");
    insert_skill(&store, &v2, Timestamp::now(), &CeremonyToken::test_token()).unwrap();

    // v1 mined -> promoted should be refused (higher Installed exists).
    let mut v1 = skill_with("bump", SkillProvenance::MinerDistilled, 1, "agent_a");
    install_skill(&store, &mut v1, Timestamp::now()).unwrap();
    let reviewed = run_promotion_review(&store, &get_skill(&store, "bump", 1).unwrap().unwrap())
        .expect("benign mined skill should pass review");
    let principal = Ulid::new();
    record_preview(
        &store,
        "bump",
        1,
        &principal.to_string(),
        &v1.content_digest,
        "MinerDistilled",
        "",
        "digest",
    );
    let err =
        promote_skill(&store, &reviewed, principal, &CeremonyToken::test_token()).unwrap_err();
    assert!(matches!(err, crate::store::StoreError::SkillLifecycle(_)));
    // v1 stays PendingReview; v2 remains the active shelf version.
    assert_eq!(
        get_skill(&store, "bump", 1).unwrap().unwrap().state,
        SkillState::PendingReview
    );
    assert_eq!(
        get_skill(&store, "bump", 2).unwrap().unwrap().state,
        SkillState::Installed
    );
}

#[test]
fn ceremony_token_mintable_only_via_test_token() {
    // This sibling module can use only the test-gated helper. The real
    // `CeremonyToken::new()` is private to ceremony.rs; any sibling call is
    // a compiler error (E0624), enforced by the green workspace build.
    let _token = CeremonyToken::test_token();
}
