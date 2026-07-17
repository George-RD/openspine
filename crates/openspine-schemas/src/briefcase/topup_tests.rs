use super::tests::{shape, sources};
use super::*;
use serde_json::json;
use ulid::Ulid;

// ---- top-up: digest binding + replay protection ------------------------

fn top_up_policy() -> TopUpPolicy {
    TopUpPolicy::new([(
        (
            RelationshipTier::Owner,
            TaskClass::Conversation,
            SectionKind::Skill,
        ),
        5,
    )])
}

#[test]
fn persisted_topup_request_digests_section_key_and_justification() {
    let request = TopUpRequest {
        request_id: Ulid::new(),
        section_key: "calendar".to_string(),
        kind: SectionKind::Preference,
        requested_depth: 2,
        justification: "private scheduling context".to_string(),
    };
    let persisted = request.for_persistence();
    assert_ne!(persisted.section_key, request.section_key);
    assert!(persisted.section_key.starts_with("key:sha256:"));
    assert_ne!(persisted.justification, request.justification);
    assert!(persisted.justification.starts_with("sha256:"));
}
#[test]
fn evaluate_top_up_denies_without_a_policy_rule() {
    let packed = pack(
        shape(Ulid::new()),
        &sources(),
        RelationshipTier::Owner,
        TaskClass::Conversation,
    );
    let request = TopUpRequest {
        request_id: Ulid::new(),
        section_key: "calendar".to_string(),
        kind: SectionKind::Preference,
        requested_depth: 1,
        justification: "need it".to_string(),
    };
    let decision = packed.evaluate_top_up(&request, &TopUpPolicy::default());
    assert!(matches!(decision.outcome, TopUpOutcome::Denied { .. }));
}

#[test]
fn apply_top_up_requires_a_correct_digest_binding() {
    let mut packed = pack(
        shape(Ulid::new()),
        &sources(),
        RelationshipTier::Owner,
        TaskClass::Conversation,
    );
    let request = TopUpRequest {
        request_id: Ulid::new(),
        section_key: "advanced_email_drafting".to_string(),
        kind: SectionKind::Skill,
        requested_depth: 3,
        justification: "long thread".to_string(),
    };
    let decision = packed.evaluate_top_up(&request, &top_up_policy());
    assert!(matches!(decision.outcome, TopUpOutcome::Allowed));

    // No digest at all: refused (fail closed on an unbound Allowed decision).
    let source = SourceSlice {
        key: request.section_key.clone(),
        payload: json!({"skill": "advanced_email_drafting"}),
        minimum_depth: 1,
    };
    let err = packed
        .clone()
        .apply_top_up(decision.clone(), source.clone(), &top_up_policy())
        .unwrap_err();
    assert_eq!(err, BriefcaseError::TopUpSourceMismatch);

    // Wrong digest: refused.
    let mut wrong = decision.clone();
    wrong.source_digest = Some(crate::digest::digest_of(&json!({"skill": "different"})));
    let err = packed
        .clone()
        .apply_top_up(wrong, source.clone(), &top_up_policy())
        .unwrap_err();
    assert_eq!(err, BriefcaseError::TopUpSourceMismatch);

    // Correct digest: applied, and the section lands with WorkerScratch
    // visibility at the requested depth.
    let mut bound = decision.clone();
    bound.source_digest = Some(crate::digest::digest_of(&source.payload));
    packed
        .apply_top_up(bound, source, &top_up_policy())
        .unwrap();
    let applied = packed
        .sections
        .iter()
        .find(|s| s.key.ends_with("advanced_email_drafting"))
        .expect("top-up section applied");
    assert_eq!(applied.visibility, VisibilityClass::WorkerScratch);
    assert_eq!(applied.depth, 3);
}

#[test]
fn apply_top_up_rejects_replay_of_an_already_decided_request() {
    let mut packed = pack(
        shape(Ulid::new()),
        &sources(),
        RelationshipTier::Owner,
        TaskClass::Conversation,
    );
    let request = TopUpRequest {
        request_id: Ulid::new(),
        section_key: "advanced_email_drafting".to_string(),
        kind: SectionKind::Skill,
        requested_depth: 2,
        justification: "reused id".to_string(),
    };
    let source = SourceSlice {
        key: request.section_key.clone(),
        payload: json!({"skill": "advanced_email_drafting"}),
        minimum_depth: 1,
    };
    let mut decision = packed.evaluate_top_up(&request, &top_up_policy());
    decision.source_digest = Some(crate::digest::digest_of(&source.payload));
    packed
        .apply_top_up(decision.clone(), source.clone(), &top_up_policy())
        .unwrap();

    // Replaying the exact same request_id — even with a validly bound
    // digest — must be rejected, not silently re-applied or treated as a
    // fresh success.
    let err = packed
        .apply_top_up(decision, source, &top_up_policy())
        .unwrap_err();
    assert_eq!(err, BriefcaseError::TopUpReplay(request.request_id));
}

#[test]
fn apply_top_up_refuses_a_denied_decision() {
    let mut packed = pack(
        shape(Ulid::new()),
        &sources(),
        RelationshipTier::Owner,
        TaskClass::Conversation,
    );
    let request = TopUpRequest {
        request_id: Ulid::new(),
        section_key: "calendar".to_string(),
        kind: SectionKind::Preference,
        requested_depth: 1,
        justification: "need it".to_string(),
    };
    let decision = packed.evaluate_top_up(&request, &TopUpPolicy::default());
    let source = SourceSlice {
        key: request.section_key.clone(),
        payload: json!({}),
        minimum_depth: 1,
    };
    let err = packed
        .apply_top_up(decision, source, &top_up_policy())
        .unwrap_err();
    assert_eq!(err, BriefcaseError::TopUpNotAllowed("calendar".to_string()));
}

#[test]
fn record_top_up_decision_is_gate_visible_in_the_log() {
    let mut packed = pack(
        shape(Ulid::new()),
        &sources(),
        RelationshipTier::Owner,
        TaskClass::Conversation,
    );
    let request = TopUpRequest {
        request_id: Ulid::new(),
        section_key: "calendar".to_string(),
        kind: SectionKind::Preference,
        requested_depth: 1,
        justification: "need it".to_string(),
    };
    let decision = packed.evaluate_top_up(&request, &TopUpPolicy::default());
    packed.record_top_up_decision(decision.clone());
    assert_eq!(packed.top_up_log, vec![decision]);
}

#[test]
fn depth_limits_packed_content_for_strangers() {
    let pool = vec![
        LearnedSource {
            key: "a".to_string(),
            kind: SectionKind::Preference,
            payload: json!({}),
            applicable_tiers: vec![],
            applicable_workflows: vec![],
        },
        LearnedSource {
            key: "b".to_string(),
            kind: SectionKind::Preference,
            payload: json!({}),
            applicable_tiers: vec![],
            applicable_workflows: vec![],
        },
        LearnedSource {
            key: "c".to_string(),
            kind: SectionKind::Preference,
            payload: json!({}),
            applicable_tiers: vec![],
            applicable_workflows: vec![],
        },
    ];
    let (preferences, _) = select_relevant_sources(&pool, "wf", RelationshipTier::Stranger);
    // Stranger × Conversation = depth 1: only 1 preference packed.
    let packed = pack(
        shape(Ulid::new()),
        &PackSources {
            grant_view: json!({}),
            preferences,
            skills: vec![],
            counterparty_slice: json!({}),
        },
        RelationshipTier::Stranger,
        TaskClass::Conversation,
    );
    let pref_count = packed
        .sections
        .iter()
        .filter(|s| s.kind == SectionKind::Preference)
        .count();
    assert_eq!(pref_count, 1);
}

#[test]
fn apply_top_up_updates_in_place_when_section_already_packed() {
    let mut packed = pack(
        shape(Ulid::new()),
        &sources(),
        RelationshipTier::Owner,
        TaskClass::Conversation,
    );
    let request = TopUpRequest {
        request_id: Ulid::new(),
        section_key: "tone".to_string(),
        kind: SectionKind::Preference,
        requested_depth: 5,
        justification: "deeper".to_string(),
    };
    let source = SourceSlice {
        key: request.section_key.clone(),
        payload: json!({"tone": "verbose"}),
        minimum_depth: 1,
    };
    let mut decision = packed.evaluate_top_up(&request, &top_up_policy_relevant());
    decision.source_digest = Some(crate::digest::digest_of(&source.payload));
    packed
        .apply_top_up(decision, source, &top_up_policy_relevant())
        .unwrap();
    let matching: Vec<_> = packed
        .sections
        .iter()
        .filter(|s| s.key.ends_with("tone"))
        .collect();
    assert_eq!(matching.len(), 1, "no duplicate sections for the same key");
    assert_eq!(matching[0].depth, 5);
    assert_eq!(matching[0].payload, json!({"tone": "verbose"}));
}

fn top_up_policy_relevant() -> TopUpPolicy {
    TopUpPolicy::new([
        (
            (
                RelationshipTier::Owner,
                TaskClass::Conversation,
                SectionKind::Skill,
            ),
            5,
        ),
        (
            (
                RelationshipTier::Owner,
                TaskClass::Conversation,
                SectionKind::Preference,
            ),
            5,
        ),
    ])
}

#[test]
fn counterparty_ref_unresolved_serializes_with_tag() {
    let cp = CounterpartyRef::Unresolved {
        channel: "email".to_string(),
        identifier: "thread-123".to_string(),
    };
    let v = serde_json::to_value(&cp).unwrap();
    assert_eq!(v["kind"], "unresolved");
    assert_eq!(v["channel"], "email");
    assert_eq!(v["identifier"], "thread-123");
    assert_eq!(cp.tier(), RelationshipTier::Stranger);
}

#[test]
fn repeated_shallow_topups_cannot_exceed_aggregate_depth_budget() {
    let mut packed = pack(
        shape(Ulid::new()),
        &PackSources {
            grant_view: json!({}),
            preferences: vec![],
            skills: vec![],
            counterparty_slice: json!({}),
        },
        RelationshipTier::Stranger,
        TaskClass::Conversation,
    );
    let policy = TopUpPolicy::new([(
        (
            RelationshipTier::Stranger,
            TaskClass::Conversation,
            SectionKind::Skill,
        ),
        1,
    )]);
    for (key, rank) in [("first", 1), ("second", 2)] {
        let request = TopUpRequest {
            request_id: Ulid::new(),
            section_key: key.to_string(),
            kind: SectionKind::Skill,
            requested_depth: 1,
            justification: "need one more".to_string(),
        };
        let source = SourceSlice {
            key: key.to_string(),
            payload: json!({"key": key}),
            minimum_depth: 1,
        };
        let mut decision = packed.evaluate_top_up(&request, &policy);
        decision.source_digest = Some(crate::digest::digest_of(&source.payload));
        if rank == 1 {
            packed.apply_top_up(decision, source, &policy).unwrap();
        } else {
            // The second top-up is denied at evaluation time by the aggregate
            // ceiling (max_total_sections defaults to self.depth=1, and the
            // briefcase already has 1 skill section).
            assert!(matches!(decision.outcome, TopUpOutcome::Denied { .. }));
        }
    }
}

#[test]
fn ranked_source_requires_matching_requested_depth() {
    let mut packed = pack(
        shape(Ulid::new()),
        &PackSources {
            grant_view: json!({}),
            preferences: vec![],
            skills: vec![],
            counterparty_slice: json!({}),
        },
        RelationshipTier::Stranger,
        TaskClass::Conversation,
    );
    let request = TopUpRequest {
        request_id: Ulid::new(),
        section_key: "ranked".to_string(),
        kind: SectionKind::Skill,
        requested_depth: 1,
        justification: "rank check".to_string(),
    };
    let source = SourceSlice {
        key: "ranked".to_string(),
        payload: json!({"rank": 2}),
        minimum_depth: 2,
    };
    let policy = TopUpPolicy::new([(
        (
            RelationshipTier::Stranger,
            TaskClass::Conversation,
            SectionKind::Skill,
        ),
        1,
    )]);
    let mut decision = packed.evaluate_top_up(&request, &policy);
    decision.source_digest = Some(crate::digest::digest_of(&source.payload));
    assert_eq!(
        packed.apply_top_up(decision, source, &policy),
        Err(BriefcaseError::TopUpDepthExceeded)
    );
}
