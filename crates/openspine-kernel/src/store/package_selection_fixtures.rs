//! Shared fixtures for the package-selection Store tests (ANO-822).
use super::learned_artifacts::{
    CompatibilityStatus, LearnedArtifact, NominationStatus, Provenance,
};
use super::package_install::{CommitResult, PrepareResult};
use super::{Store, StoreError};
use crate::package::install_types::{InstallReceipt, PackageIdentity, PackageProvenance};
use crate::package::selection_types::{
    SelectionDecision, SelectionRefusal, SelectionSource, SelectionSourceMode, SelectionTarget,
};
use jiff::{SignedDuration, Timestamp};
use openspine_schemas::artifact::ArtifactNamespace;
use openspine_schemas::digest::digest_of_bytes;
use ulid::Ulid;

pub(super) fn identity(revision: u32) -> PackageIdentity {
    PackageIdentity {
        package_id: "lyra".into(),
        revision,
        inventory_format_version: 1,
        content_digest: digest_of_bytes(format!("content-{revision}").as_bytes()),
        manifest_digest: digest_of_bytes(format!("manifest-{revision}").as_bytes()),
    }
}

pub(super) fn install(store: &Store, revision: u32) -> InstallReceipt {
    let PrepareResult::Prepared(metadata) = store
        .prepare_package_install(identity(revision), PackageProvenance::LocalUnverified)
        .unwrap()
    else {
        panic!("fresh revision prepares");
    };
    let CommitResult::Committed(receipt) = store.commit_package_install(&metadata).unwrap() else {
        panic!("prepared install commits");
    };
    receipt
}

pub(super) fn decision(
    store: &Store,
    target: &InstallReceipt,
    now: Timestamp,
) -> SelectionDecision {
    let current = store.package_selection().unwrap();
    let source = match &current {
        None => SelectionSource {
            mode: SelectionSourceMode::LegacyConfigured,
            identity: identity(99),
            installation_id: None,
        },
        Some(record) => SelectionSource {
            mode: SelectionSourceMode::Managed,
            identity: store
                .installed_packages()
                .unwrap()
                .into_iter()
                .find(|receipt| receipt.installation_id == record.installation_id)
                .unwrap()
                .identity(),
            installation_id: Some(record.installation_id),
        },
    };
    SelectionDecision {
        schema_version: 1,
        decision_id: Ulid::new(),
        owner_binding: "hmac-sha256:owner".into(),
        principal: "owner-principal".into(),
        source,
        target: SelectionTarget {
            installation_id: target.installation_id,
            identity: target.identity(),
        },
        plan_digest: digest_of_bytes(b"plan"),
        expected_generation: current.map_or(0, |record| record.generation),
        runtime_compatibility: digest_of_bytes(b"runtime"),
        control_fingerprint: digest_of_bytes(b"controls"),
        base_epoch_changed: false,
        reconfirmations: vec![],
        presented_at: now,
        expires_at: now + SignedDuration::from_mins(10),
    }
}

pub(super) fn select(store: &Store, target: &InstallReceipt, now: Timestamp) {
    let decision = decision(store, target, now);
    store
        .record_package_selection_decision(&decision, now)
        .unwrap();
    store.commit_package_selection(&decision, now).unwrap();
}

pub(super) fn refusal(error: StoreError) -> SelectionRefusal {
    match error {
        StoreError::PackageSelection(reason) => reason,
        other => panic!("expected a selection refusal, got {other:?}"),
    }
}

pub(super) fn learned(kind: &str, id: &str, status: CompatibilityStatus) -> LearnedArtifact {
    LearnedArtifact {
        kind: kind.into(),
        artifact_id: id.into(),
        version: 1,
        namespace: ArtifactNamespace::Overlay,
        provenance: Provenance::LegacyMigration {
            discovered_at: Timestamp::now(),
        },
        accepted_via: None,
        learned_at: Timestamp::now(),
        compatibility: status,
        nomination: NominationStatus::None,
        pending_reconfirmation_id: None,
        pending_yaml_digest: None,
        accepted_dependency_fingerprint: None,
        source_path: None,
        accepted_base_epoch: Some("epoch-a".into()),
    }
}

pub(super) fn status_of(store: &Store, id: &str) -> CompatibilityStatus {
    store
        .list_learned_artifacts()
        .unwrap()
        .into_iter()
        .find(|row| row.artifact_id == id)
        .unwrap()
        .compatibility
}
