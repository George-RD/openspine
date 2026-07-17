//! Pure-function tests for the schemas-crate briefcase module (AD-021/
//! AD-031/AD-032/AD-121): pack determinism, visibility-class enforcement,
//! and the top-up digest-binding/replay guard. Kernel-owned, store-backed
//! orchestration is tested separately in `openspine-kernel::briefcase`.

use serde_json::json;
use ulid::Ulid;

use super::*;
use crate::identity::RelationshipKind;

pub(super) fn shape(counterparty_id: Ulid) -> TaskShape {
    TaskShape {
        route_id: "owner_telegram_route".to_string(),
        workflow_id: "owner_control_workflow".to_string(),
        counterparty: CounterpartyRef::Bound {
            identity_id: counterparty_id,
            relationship: RelationshipKind::Owner,
        },
    }
}

pub(super) fn sources() -> PackSources {
    PackSources {
        grant_view: json!({"allowed_actions": ["openspine.status.read"]}),
        preferences: vec![SourceSlice {
            key: "tone".to_string(),
            payload: json!({"tone": "concise"}),
            minimum_depth: 1,
        }],
        skills: vec![SourceSlice {
            key: "email_drafting".to_string(),
            payload: json!({"skill": "email_drafting"}),
            minimum_depth: 1,
        }],
        counterparty_slice: json!({"display_name": "Owner"}),
    }
}

// ---- pack determinism ------------------------------------------------

#[test]
fn identical_shape_and_sources_yield_byte_identical_pack() {
    let id = Ulid::new();
    let a = pack(
        shape(id),
        &sources(),
        RelationshipTier::Owner,
        TaskClass::Conversation,
    );
    let b = pack(
        shape(id),
        &sources(),
        RelationshipTier::Owner,
        TaskClass::Conversation,
    );
    assert_eq!(a.canonical_bytes(), b.canonical_bytes());
    assert_eq!(a, b);
}

#[test]
fn different_source_snapshot_breaks_byte_identity() {
    let id = Ulid::new();
    let a = pack(
        shape(id),
        &sources(),
        RelationshipTier::Owner,
        TaskClass::Conversation,
    );
    let mut other = sources();
    other.preferences[0].payload = json!({"tone": "verbose"});
    let b = pack(
        shape(id),
        &other,
        RelationshipTier::Owner,
        TaskClass::Conversation,
    );
    assert_ne!(a.canonical_bytes(), b.canonical_bytes());
    assert_ne!(a.source_snapshot_id, b.source_snapshot_id);
}

#[test]
fn pack_sections_are_key_sorted_and_grant_is_kernel_bound() {
    let packed = pack(
        shape(Ulid::new()),
        &sources(),
        RelationshipTier::Owner,
        TaskClass::Conversation,
    );
    let keys: Vec<&str> = packed.sections.iter().map(|s| s.key.as_str()).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    let grant_section = packed.sections.iter().find(|s| s.key == "grant").unwrap();
    assert_eq!(grant_section.visibility, VisibilityClass::KernelBound);
    assert!(packed
        .sections
        .iter()
        .filter(|s| s.key != "grant")
        .all(|s| s.visibility == VisibilityClass::WorkerScratch));
}

// ---- select_relevant_sources filtering --------------------------------

#[test]
fn select_relevant_sources_filters_by_tier_and_workflow_deterministically() {
    let pool = vec![
        LearnedSource {
            key: "b_pref".to_string(),
            kind: SectionKind::Preference,
            payload: json!({}),
            applicable_tiers: vec![RelationshipTier::Owner],
            applicable_workflows: vec![],
        },
        LearnedSource {
            key: "a_pref".to_string(),
            kind: SectionKind::Preference,
            payload: json!({}),
            applicable_tiers: vec![RelationshipTier::Stranger],
            applicable_workflows: vec![],
        },
        LearnedSource {
            key: "wrong_workflow_skill".to_string(),
            kind: SectionKind::Skill,
            payload: json!({}),
            applicable_tiers: vec![],
            applicable_workflows: vec!["other_workflow".to_string()],
        },
        LearnedSource {
            key: "a_skill".to_string(),
            kind: SectionKind::Skill,
            payload: json!({}),
            applicable_tiers: vec![],
            applicable_workflows: vec!["owner_control_workflow".to_string()],
        },
    ];
    let (preferences, skills) =
        select_relevant_sources(&pool, "owner_control_workflow", RelationshipTier::Owner);
    assert_eq!(
        preferences
            .iter()
            .map(|s| s.key.as_str())
            .collect::<Vec<_>>(),
        vec!["b_pref"]
    );
    assert_eq!(
        skills.iter().map(|s| s.key.as_str()).collect::<Vec<_>>(),
        vec!["a_skill"]
    );
}

// ---- depth ------------------------------------------------------------

#[test]
fn depth_grows_with_tier_and_class() {
    assert!(
        depth(RelationshipTier::Owner, TaskClass::Effectful)
            > depth(RelationshipTier::Stranger, TaskClass::Conversation)
    );
    assert_eq!(
        depth(RelationshipTier::Stranger, TaskClass::Conversation),
        1
    );
}

#[test]
fn same_pool_stranger_and_owner_pack_preference_and_skill_depth() {
    let pool = PackSources {
        grant_view: json!({"allowed_actions": ["openspine.status.read"]}),
        preferences: vec![
            SourceSlice {
                key: "tone".to_string(),
                payload: json!({"tone": "concise"}),
                minimum_depth: 1,
            },
            SourceSlice {
                key: "style".to_string(),
                payload: json!({"style": "formal"}),
                minimum_depth: 2,
            },
        ],
        skills: vec![
            SourceSlice {
                key: "email".to_string(),
                payload: json!({"skill": "email"}),
                minimum_depth: 1,
            },
            SourceSlice {
                key: "calendar".to_string(),
                payload: json!({"skill": "calendar"}),
                minimum_depth: 2,
            },
        ],
        counterparty_slice: json!({"display_name": "counterparty"}),
    };
    let stranger = pack(
        shape(Ulid::new()),
        &pool,
        RelationshipTier::Stranger,
        TaskClass::Conversation,
    );
    let owner = pack(
        shape(Ulid::new()),
        &pool,
        RelationshipTier::Owner,
        TaskClass::Effectful,
    );
    assert!(stranger.sections.len() < owner.sections.len());
    assert!(stranger.sections.iter().all(|section| section.depth == 1));
    assert!(owner
        .sections
        .iter()
        .any(|section| section.key == "preference:style"));
    assert!(owner
        .sections
        .iter()
        .any(|section| section.key == "skill:calendar"));
}

// ---- visibility-class enforcement --------------------------------------

#[test]
fn kernel_bound_section_never_reaches_worker_view_even_if_allowed_lists_it() {
    let packed = pack(
        shape(Ulid::new()),
        &sources(),
        RelationshipTier::Owner,
        TaskClass::Conversation,
    );
    let worker_id = Ulid::new();
    let mut visibility = WorkerVisibility::worker_default(worker_id);
    // Attacker/bug scenario: even if KernelBound is explicitly added to the
    // allowed set, `view_for` must still exclude it structurally.
    visibility.allowed.insert(VisibilityClass::KernelBound);
    let view = packed.view_for(&visibility);
    assert!(view.sections.iter().all(|s| s.kind != SectionKind::Grant));
    assert!(view.sections.iter().all(|s| s.key != "grant"));
}

#[test]
fn worker_default_visibility_excludes_returned_output_and_kernel_bound() {
    let mut packed = pack(
        shape(Ulid::new()),
        &sources(),
        RelationshipTier::Owner,
        TaskClass::Conversation,
    );
    packed.sections.push(BriefcaseSection {
        key: "outcome".to_string(),
        kind: SectionKind::CounterpartySlice,
        visibility: VisibilityClass::ReturnedOutput,
        depth: 1,
        payload: json!({"outcome": "done"}),
    });
    let visibility = WorkerVisibility::worker_default(Ulid::new());
    let view = packed.view_for(&visibility);
    assert!(view.sections.iter().all(|s| s.key != "outcome"));
    assert!(view.sections.iter().all(|s| s.key != "grant"));
}

#[test]
fn returned_output_export_refuses_worker_scratch_and_kernel_bound_keys() {
    let packed = pack(
        shape(Ulid::new()),
        &sources(),
        RelationshipTier::Owner,
        TaskClass::Conversation,
    );
    let err = packed
        .export_returned_output(&["grant".to_string()])
        .unwrap_err();
    assert_eq!(
        err,
        BriefcaseError::VisibilityViolation {
            key: "grant".to_string(),
            actual: VisibilityClass::KernelBound,
        }
    );
    let err = packed
        .export_returned_output(&["preference:tone".to_string()])
        .unwrap_err();
    assert_eq!(
        err,
        BriefcaseError::VisibilityViolation {
            key: "preference:tone".to_string(),
            actual: VisibilityClass::WorkerScratch,
        }
    );
}
