use super::*;

#[test]
fn activated_standing_rule_becomes_unusable_after_source_scope_erasure() {
    use crate::standing_rules_gate::consult_standing_rule_gate;
    use crate::store::standing_rules_tests::manifest;
    use openspine_schemas::action::ActionId;
    use openspine_schemas::standing_rule::BudgetWindow;

    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();
    let operations = operations_for(dir.path(), master_key());

    let counterparty = Ulid::new();
    let other = Ulid::new();
    let payload_ref = artifacts.put_scoped(counterparty, b"rule source").unwrap();
    let other_payload = artifacts.put_scoped(other, b"other rule source").unwrap();

    let erased_rule = learned_row(
        "standing_rule",
        "rule-erased",
        1,
        &payload_ref,
        counterparty,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    let kept_rule = learned_row(
        "standing_rule",
        "rule-kept",
        1,
        &other_payload,
        other,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    store.record_learned_artifact(&erased_rule).unwrap();
    store.record_learned_artifact(&kept_rule).unwrap();

    let now = jiff::Timestamp::now();
    let erased_manifest = manifest(
        "rule-erased",
        "connector.enable",
        3600,
        BudgetWindow {
            max: 5,
            window_secs: 3600,
        },
        BudgetWindow {
            max: 5,
            window_secs: 3600,
        },
        None,
    );
    let kept_manifest = manifest(
        "rule-kept",
        "timer.schedule",
        3600,
        BudgetWindow {
            max: 5,
            window_secs: 3600,
        },
        BudgetWindow {
            max: 5,
            window_secs: 3600,
        },
        None,
    );
    store
        .activate_standing_rule(&erased_manifest, None, now)
        .unwrap();
    store
        .activate_standing_rule(&kept_manifest, None, now)
        .unwrap();

    let erased_action = ActionId::new("connector.enable");
    let kept_action = ActionId::new("timer.schedule");
    let before = consult_standing_rule_gate(&store, &erased_action, now, None).unwrap();
    assert!(before.matched && before.allow, "rule usable before erase");
    assert!(store
        .active_standing_rule_for_action(&erased_action, now)
        .unwrap()
        .is_some());

    let report = erase_counterparty(&store, &artifacts, &operations, counterparty).unwrap();
    assert_eq!(report.derived_artifacts_invalidated, 1);
    assert_eq!(
        report.invalidated_identities,
        vec![crate::store::learned_artifacts::LearnedArtifactIdentity {
            kind: "standing_rule".into(),
            artifact_id: "rule-erased".into(),
            version: 1,
        }]
    );

    let after = consult_standing_rule_gate(&store, &erased_action, now, None).unwrap();
    assert!(
        !after.matched && !after.allow,
        "erased-scope standing rule must not match gate consultation"
    );
    assert!(
        store
            .active_standing_rule_for_action(&erased_action, now)
            .unwrap()
            .is_none(),
        "revoked runtime standing_rules row is invisible to active lookup"
    );
    let status: String = store
        .conn
        .lock()
        .query_row(
            "SELECT status FROM standing_rules WHERE artifact_id = ?1 AND version = 1",
            rusqlite::params!["rule-erased"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "revoked");

    // Unrelated scope keeps its activated rule.
    let kept = consult_standing_rule_gate(&store, &kept_action, now, None).unwrap();
    assert!(kept.matched && kept.allow);
    assert!(store
        .active_standing_rule_for_action(&kept_action, now)
        .unwrap()
        .is_some());
}

#[test]
fn active_model_swap_disappears_from_active_ids_after_source_scope_erasure() {
    use crate::store::proposed_artifacts::ProposedArtifact;
    use openspine_schemas::artifact::Lifecycle;

    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(dir.path().join("artifacts"), master_key()).unwrap();
    let operations = operations_for(dir.path(), master_key());

    let counterparty = Ulid::new();
    let other = Ulid::new();
    let payload_ref = artifacts.put_scoped(counterparty, b"swap source").unwrap();
    let other_payload = artifacts.put_scoped(other, b"other swap source").unwrap();

    let erased_swap = learned_row(
        "model_swap",
        "swap-erased",
        1,
        &payload_ref,
        counterparty,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    let kept_swap = learned_row(
        "model_swap",
        "swap-kept",
        1,
        &other_payload,
        other,
        crate::store::learned_artifacts::CompatibilityStatus::Compatible,
    );
    store.record_learned_artifact(&erased_swap).unwrap();
    store.record_learned_artifact(&kept_swap).unwrap();

    let erased_yaml = artifacts.put(b"erased-swap-yaml").unwrap();
    let other_version_yaml = artifacts.put(b"erased-swap-yaml-v2").unwrap();
    let kept_yaml = artifacts.put(b"kept-swap-yaml").unwrap();
    let erased_proposal = Ulid::new();
    let other_version_proposal = Ulid::new();
    let kept_proposal = Ulid::new();

    // Only v1 is Active for the erased identity — this is the recovery
    // input that must leave active_model_swap_ids after source-scope erase.
    store
        .insert_proposed_artifact(&ProposedArtifact {
            id: erased_proposal,
            kind: "model_swap".to_string(),
            artifact_id: "swap-erased".to_string(),
            version: 1,
            state: Lifecycle::Proposed,
            yaml_digest: erased_yaml.digest.as_str().to_string(),
            task_grant_id: Ulid::new(),
            action_request_id: None,
            proposed_at: jiff::Timestamp::now(),
            lineage: None,
        })
        .unwrap();
    store
        .force_proposed_artifact_state_for_test(erased_proposal, Lifecycle::Active)
        .unwrap();

    // Same artifact_id, different version, Active but not in the invalidated
    // identity set (no matching learned provenance for v2). Exact-identity
    // retirement must leave it Active.
    store
        .insert_proposed_artifact(&ProposedArtifact {
            id: other_version_proposal,
            kind: "model_swap".to_string(),
            artifact_id: "swap-erased".to_string(),
            version: 2,
            state: Lifecycle::Proposed,
            yaml_digest: other_version_yaml.digest.as_str().to_string(),
            task_grant_id: Ulid::new(),
            action_request_id: None,
            proposed_at: jiff::Timestamp::now(),
            lineage: None,
        })
        .unwrap();
    store
        .force_proposed_artifact_state_for_test(other_version_proposal, Lifecycle::Active)
        .unwrap();

    store
        .insert_proposed_artifact(&ProposedArtifact {
            id: kept_proposal,
            kind: "model_swap".to_string(),
            artifact_id: "swap-kept".to_string(),
            version: 1,
            state: Lifecycle::Proposed,
            yaml_digest: kept_yaml.digest.as_str().to_string(),
            task_grant_id: Ulid::new(),
            action_request_id: None,
            proposed_at: jiff::Timestamp::now(),
            lineage: None,
        })
        .unwrap();
    store
        .force_proposed_artifact_state_for_test(kept_proposal, Lifecycle::Active)
        .unwrap();

    // Force the disappearance case: only the invalidated version is Active
    // for swap-erased when we observe active_model_swap_ids. Temporarily
    // demote v2, assert disappearance, then re-check version scoping on
    // a second Active same-id row that is not in matching identities.
    store
        .force_proposed_artifact_state_for_test(other_version_proposal, Lifecycle::Approved)
        .unwrap();

    let before = store.active_model_swap_ids().unwrap();
    assert!(before.contains(&("swap-erased".to_string(), 1)));
    assert!(before.contains(&("swap-kept".to_string(), 1)));

    let report = erase_counterparty(&store, &artifacts, &operations, counterparty).unwrap();
    assert_eq!(report.derived_artifacts_invalidated, 1);
    assert_eq!(
        report.invalidated_identities,
        vec![crate::store::learned_artifacts::LearnedArtifactIdentity {
            kind: "model_swap".into(),
            artifact_id: "swap-erased".into(),
            version: 1,
        }]
    );

    let after = store.active_model_swap_ids().unwrap();
    assert!(
        !after.iter().any(|(id, _)| id == "swap-erased"),
        "erased-scope active model_swap must leave active_model_swap_ids"
    );
    assert!(
        after.contains(&("swap-kept".to_string(), 1)),
        "unrelated active model_swap must remain visible to recovery"
    );
    assert_eq!(
        store
            .find_proposed_artifact_state("model_swap", "swap-erased", 1)
            .unwrap()
            .map(|(state, _)| state),
        Some(Lifecycle::Retired)
    );

    // Version-scoped: promote same-id v2 (not in matching identities) and
    // re-run erase. Exact identity matching must not retire v2.
    store
        .force_proposed_artifact_state_for_test(other_version_proposal, Lifecycle::Active)
        .unwrap();
    assert!(store
        .active_model_swap_ids()
        .unwrap()
        .contains(&("swap-erased".to_string(), 2)));
    let retry = erase_counterparty(&store, &artifacts, &operations, counterparty).unwrap();
    assert_eq!(retry.derived_artifacts_invalidated, 0);
    assert_eq!(
        store
            .find_proposed_artifact_state("model_swap", "swap-erased", 2)
            .unwrap()
            .map(|(state, _)| state),
        Some(Lifecycle::Active),
        "same artifact_id different version must not be retired"
    );
    assert_eq!(
        store
            .find_proposed_artifact_state("model_swap", "swap-kept", 1)
            .unwrap()
            .map(|(state, _)| state),
        Some(Lifecycle::Active)
    );
}
