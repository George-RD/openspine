use jiff::Timestamp;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::lineage::ArtifactLineage;
use ulid::Ulid;

use crate::store::proposed_artifacts::ProposedArtifact;
use crate::store::Store;

#[test]
fn generic_review_transition_is_rejected_without_gate_proofs() {
    let store = Store::open_in_memory().unwrap();
    let err = store
        .set_proposed_artifact_state(Ulid::new(), Lifecycle::Validated, Lifecycle::ReviewRequired)
        .unwrap_err();
    assert!(err.to_string().contains("AD-142"));
}

#[test]
fn direct_review_required_insert_is_rejected() {
    let store = Store::open_in_memory().unwrap();
    let row = ProposedArtifact {
        id: Ulid::new(),
        kind: "route".to_string(),
        artifact_id: "forged".to_string(),
        version: 1,
        state: Lifecycle::ReviewRequired,
        yaml_digest: format!("sha256:{}", "0".repeat(64)),
        task_grant_id: Ulid::new(),
        action_request_id: Some(Ulid::new()),
        proposed_at: Timestamp::now(),
        lineage: Some(ArtifactLineage::root()),
    };
    let err = store.insert_proposed_artifact(&row).unwrap_err();
    assert!(err.to_string().contains("proposed state"));
}
