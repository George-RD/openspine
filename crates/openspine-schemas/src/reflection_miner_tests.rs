use super::*;
use crate::action::ActionId;
use crate::artifact::ArtifactRef;
use crate::digest::Digest;
use crate::grant::GrantMode;
use crate::persona::PersonaElement;
use jiff::Timestamp;

fn exchange() -> ArtifactRef {
    ArtifactRef {
        digest: Digest::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
        schema_version: 1,
    }
}

fn source_event() -> Ulid {
    Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap()
}

fn provenance() -> ReflectionProvenance {
    ReflectionProvenance {
        source_event_id: source_event(),
        source_exchange: exchange(),
    }
}

fn grant() -> TaskGrant {
    let now = Timestamp::now();
    let id = Ulid::new();
    TaskGrant {
        id,
        schema_version: 1,
        lifecycle_state: Lifecycle::Active,
        user: "owner".into(),
        purpose: "scheduled reflection miner".into(),
        issued_by: "kernel".into(),
        issued_at: now,
        expires_at: now + std::time::Duration::from_secs(60),
        event_id: Ulid::new(),
        route_id: "reflection_route".into(),
        agent_id: "reflection_miner".into(),
        workflow_id: "reflection_workflow".into(),
        capability_pack_id: "reflection_pack".into(),
        authority_sources: vec![],
        selection_tokens: vec![],
        allowed_actions: vec![ActionId::new("model.generate:approved_provider")],
        approval_required_actions: vec![],
        denied_actions: vec![],
        allowed_egress_classes: vec![],
        output_channels: vec![],
        limits: GrantLimits {
            max_model_calls: 1,
            max_artifacts: 4,
            max_runtime_seconds: 60,
        },
        task_token: "token".into(),
        root_grant_id: id,
        parent_grant_id: None,
        mode: GrantMode::Live,
        chain: vec![],
        caveat_mac: String::new(),
        thread_id: None,
        persona_id: None,
    }
}

/// Kernel-shaped briefcase: every entry carries an encrypted exchange ref and
/// a real `artifact_id`. The repeated-approval evidence (two `short_replies`
/// rows) and the consolidation targets (`micro-*`/`dead-*`) are present so the
/// miner can derive counts from the slice rather than a caller argument.
fn briefcase(id: Ulid) -> MinerBriefcase {
    MinerBriefcase::scoped(
        id,
        "owner-conversation:42",
        vec![
            AuditTrailEntry {
                scope: "owner-conversation:42".into(),
                artifact_id: "digest_brief_default".into(),
                event_id: source_event(),
                exchange: exchange(),
                classification: DataClassification::Private,
            },
            AuditTrailEntry {
                scope: "owner-conversation:42".into(),
                artifact_id: "short_replies".into(),
                event_id: Ulid::new(),
                exchange: exchange(),
                classification: DataClassification::Private,
            },
            AuditTrailEntry {
                scope: "owner-conversation:42".into(),
                artifact_id: "short_replies".into(),
                event_id: Ulid::new(),
                exchange: exchange(),
                classification: DataClassification::Private,
            },
            AuditTrailEntry {
                scope: "owner-conversation:42".into(),
                artifact_id: "micro-1".into(),
                event_id: Ulid::new(),
                exchange: exchange(),
                classification: DataClassification::Private,
            },
            AuditTrailEntry {
                scope: "owner-conversation:42".into(),
                artifact_id: "micro-2".into(),
                event_id: Ulid::new(),
                exchange: exchange(),
                classification: DataClassification::Private,
            },
            AuditTrailEntry {
                scope: "owner-conversation:42".into(),
                artifact_id: "dead-1".into(),
                event_id: Ulid::new(),
                exchange: exchange(),
                classification: DataClassification::Private,
            },
        ],
    )
    .unwrap()
}

fn admitted() -> OrdinaryMinerGrant {
    let g = grant();
    OrdinaryMinerGrant::admit(&g, &Constraints::default(), briefcase(g.id)).unwrap()
}

#[test]
fn ordinary_grant_has_empty_output_channels_and_scoped_audit_slice() {
    let g = grant();
    let miner = OrdinaryMinerGrant::admit(&g, &Constraints::default(), briefcase(g.id)).unwrap();
    assert!(miner
        .briefcase
        .entries()
        .iter()
        .all(|entry| entry.scope == miner.briefcase.scope));
}

#[test]
fn miner_grant_rejects_direct_mutation_actions() {
    let mut g = grant();
    g.allowed_actions
        .push(ActionId::new("standing_rule.activate"));
    assert_eq!(
        OrdinaryMinerGrant::admit(&g, &Constraints::default(), briefcase(g.id)),
        Err(MinerError::DirectMutationAction)
    );
}

#[test]
fn miner_grant_rejects_output_channels() {
    let mut g = grant();
    g.output_channels.push("telegram.owner.reply".into());
    assert_eq!(
        OrdinaryMinerGrant::admit(&g, &Constraints::default(), briefcase(g.id)),
        Err(MinerError::OutputChannelsNotEmpty)
    );
}

#[test]
fn miner_cannot_write_kernel_state_and_only_returns_proposed_rows() {
    let miner = admitted();
    let observation = ReflectionObservation::StatedPreference(PreferenceObservation {
        kind: "persona".into(),
        artifact_id: "tone".into(),
        version: 1,
        statement: "Use concise wording".into(),
        provenance: provenance(),
    });
    let proposals = ReflectionMiner.mine(&miner, &[observation]).unwrap();
    assert_eq!(proposals[0].lifecycle_state, Lifecycle::Proposed);
    assert_eq!(proposals[0].class, ReflectionOutputClass::StatedPreferences);
}

#[test]
fn every_miner_proposal_carries_encrypted_exchange_provenance() {
    let miner = admitted();
    let source = provenance();
    let observation = ReflectionObservation::StatedPreference(PreferenceObservation {
        kind: "persona".into(),
        artifact_id: "tone".into(),
        version: 1,
        statement: "Use concise wording".into(),
        provenance: source.clone(),
    });
    let proposal = ReflectionMiner
        .mine(&miner, &[observation])
        .unwrap()
        .remove(0);
    assert_eq!(proposal.provenance, source);
}

#[test]
fn correction_rewrites_instruction_and_negative_constraint_becomes_probe() {
    let miner = admitted();
    let correction = CorrectionObservation {
        kind: "persona".into(),
        artifact_id: "reply_style".into(),
        version: 2,
        instruction: "Draft replies in at most three sentences".into(),
        reason: "Long replies bury the requested action".into(),
        negative_constraint: Some("Never bury the requested action".into()),
        provenance: provenance(),
    };
    let proposal = ReflectionMiner
        .mine(&miner, &[ReflectionObservation::Correction(correction)])
        .unwrap()
        .remove(0);
    let ReflectionProposalBody::InstructionRewrite {
        instruction,
        eval_probe,
        ..
    } = proposal.body
    else {
        panic!("correction must be an instruction rewrite");
    };
    assert_eq!(instruction, "Draft replies in at most three sentences");
    assert_eq!(
        eval_probe.unwrap().constraint,
        "Never bury the requested action"
    );
}

#[test]
fn correction_never_appends_a_prohibition_artifact() {
    let miner = admitted();
    let correction = CorrectionObservation {
        kind: "persona".into(),
        artifact_id: "reply_style".into(),
        version: 2,
        instruction: "Draft replies in at most three sentences".into(),
        reason: "Owner dislikes long replies".into(),
        negative_constraint: Some("Do not write long replies".into()),
        provenance: provenance(),
    };
    let proposal = ReflectionMiner
        .mine(&miner, &[ReflectionObservation::Correction(correction)])
        .unwrap()
        .remove(0);
    let ReflectionProposalBody::InstructionRewrite {
        instruction,
        reason,
        eval_probe,
    } = proposal.body
    else {
        panic!("correction must be an instruction rewrite, never a prohibition append");
    };
    assert!(!instruction.to_lowercase().starts_with("do not"));
    assert!(!reason.to_lowercase().starts_with("do not"));
    assert!(
        eval_probe.is_some(),
        "negative constraint becomes an eval probe"
    );
}

#[test]
fn correction_rejects_prohibition_shaped_instruction() {
    let miner = admitted();
    let correction = CorrectionObservation {
        kind: "persona".into(),
        artifact_id: "reply_style".into(),
        version: 2,
        instruction: "Do not write long replies".into(),
        reason: "Owner dislikes long replies".into(),
        negative_constraint: None,
        provenance: provenance(),
    };
    assert_eq!(
        ReflectionMiner.mine(&miner, &[ReflectionObservation::Correction(correction)]),
        Err(MinerError::ProhibitionShapedCorrection)
    );
}

#[test]
fn repeated_approval_is_only_a_standing_rule_candidate() {
    let miner = admitted();
    let proposal = ReflectionMiner
        .mine(
            &miner,
            &[ReflectionObservation::RepeatedApproval(
                ApprovalObservation {
                    kind: "standing_rule".into(),
                    artifact_id: "short_replies".into(),
                    version: 1,
                    action_id: "openspine.status.read".into(),
                    candidate: "Keep approved replies concise".into(),
                    provenance: provenance(),
                },
            )],
        )
        .unwrap()
        .remove(0);
    let ReflectionProposalBody::StandingRuleCandidate { action_id, .. } = &proposal.body else {
        panic!("repeated approval must be a standing-rule candidate");
    };
    assert_eq!(action_id, "openspine.status.read");
    assert_eq!(proposal.lifecycle_state, Lifecycle::Proposed);
}

#[test]
fn repeated_approval_requires_kernel_verifiable_evidence() {
    // A caller-supplied count is gone; the miner derives evidence from the
    // packed briefcase. With only one audit row for `micro-1`, no candidate.
    let miner = admitted();
    let err = ReflectionMiner
        .mine(
            &miner,
            &[ReflectionObservation::RepeatedApproval(
                ApprovalObservation {
                    kind: "standing_rule".into(),
                    artifact_id: "micro-1".into(),
                    version: 1,
                    action_id: "openspine.status.read".into(),
                    candidate: "Keep replies concise".into(),
                    provenance: provenance(),
                },
            )],
        )
        .unwrap_err();
    assert_eq!(err, MinerError::InsufficientApprovals);
}

#[test]
fn digest_default_owner_correction_uses_normal_persona_proposal_route() {
    let miner = admitted();
    let correction = CorrectionObservation::persona_digest(
        2,
        "Present a compact decision brief before detail",
        "The owner asked for a faster scan",
        None,
        provenance(),
    );
    let proposal = ReflectionMiner
        .mine(&miner, &[ReflectionObservation::Correction(correction)])
        .unwrap()
        .remove(0);
    assert_eq!(proposal.kind, "persona");
    assert_eq!(proposal.artifact_id, DIGEST_BRIEF_DEFAULT_ID);
    assert_eq!(proposal.lifecycle_state, Lifecycle::Proposed);
}

#[test]
fn consolidation_is_a_proposal_not_an_immediate_prune_or_merge() {
    let miner = admitted();
    let proposal = ReflectionMiner
        .consolidation(
            &miner,
            vec!["micro-1".into(), "micro-2".into()],
            vec!["dead-1".into()],
            provenance(),
        )
        .unwrap();
    assert_eq!(proposal.class, ReflectionOutputClass::Consolidation);
    assert_eq!(proposal.lifecycle_state, Lifecycle::Proposed);
}

#[test]
fn consolidation_rejects_targets_absent_from_briefcase() {
    let miner = admitted();
    let err = ReflectionMiner
        .consolidation(
            &miner,
            vec!["not-in-briefcase".into()],
            vec![],
            provenance(),
        )
        .unwrap_err();
    assert_eq!(err, MinerError::ConsolidationTargetNotInBriefcase);
}

#[test]
fn persona_proposal_serializes_to_normal_lifecycle_payload() {
    let miner = admitted();
    let proposal = ReflectionMiner
        .mine(
            &miner,
            &[ReflectionObservation::StatedPreference(
                PreferenceObservation {
                    kind: "persona".into(),
                    artifact_id: "tone".into(),
                    version: 1,
                    statement: "Use concise wording".into(),
                    provenance: provenance(),
                },
            )],
        )
        .unwrap()
        .remove(0);
    let payload = proposal.to_proposal_payload().unwrap();
    assert_eq!(payload["kind"], "persona");
    let element: PersonaElement = serde_yaml::from_str(payload["yaml"].as_str().unwrap()).unwrap();
    assert_eq!(element.guidance, "Use concise wording");
    assert_eq!(element.lifecycle_state, Lifecycle::Proposed);
}

#[test]
fn classification_ceiling_is_derived_from_pack_not_caller() {
    let g = grant();
    let public_entry = MinerBriefcase::scoped(
        g.id,
        "scope",
        vec![AuditTrailEntry {
            scope: "scope".into(),
            artifact_id: "x".into(),
            event_id: Ulid::new(),
            exchange: exchange(),
            classification: DataClassification::Public,
        }],
    )
    .unwrap();
    let internal_constraints = Constraints {
        data_classification_max: Some(DataClassification::Internal),
        ..Constraints::default()
    };
    assert!(OrdinaryMinerGrant::admit(&g, &internal_constraints, public_entry).is_ok());

    let internal_entry = MinerBriefcase::scoped(
        g.id,
        "scope",
        vec![AuditTrailEntry {
            scope: "scope".into(),
            artifact_id: "x".into(),
            event_id: Ulid::new(),
            exchange: exchange(),
            classification: DataClassification::Internal,
        }],
    )
    .unwrap();
    let public_constraints = Constraints {
        data_classification_max: Some(DataClassification::Public),
        ..Constraints::default()
    };
    assert_eq!(
        OrdinaryMinerGrant::admit(&g, &public_constraints, internal_entry),
        Err(MinerError::ClassificationExceeded)
    );
}

#[test]
fn miner_rejects_observation_outside_scoped_audit_slice() {
    let miner = admitted();
    let observation = ReflectionObservation::StatedPreference(PreferenceObservation {
        kind: "persona".into(),
        artifact_id: "tone".into(),
        version: 1,
        statement: "Use concise wording".into(),
        provenance: ReflectionProvenance {
            source_event_id: Ulid::new(),
            source_exchange: exchange(),
        },
    });
    assert_eq!(
        ReflectionMiner.mine(&miner, &[observation]),
        Err(MinerError::ProvenanceOutOfScope)
    );
}
