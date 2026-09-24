use super::*;

#[test]
fn package_labels_cannot_emit_terminal_or_bidi_controls() {
    let data = serde_json::json!({"id": "untrusted\u{1b}[2J\n\u{7f}\u{85}\u{202e}\u{2066}label"});
    for pretty in [false, true] {
        let output = escaped_json(&data, pretty);
        assert!(output.is_ascii());
        assert!(!output.contains('\u{1b}'));
        assert!(!output.contains('\u{7f}'));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output).unwrap(),
            data
        );
    }
}

fn review(candidate: &PackageSnapshot, installation: u128) -> ReviewReport {
    review_with_overlay(candidate, installation, None)
}

fn review_with_overlay(
    candidate: &PackageSnapshot,
    installation: u128,
    source: Option<&str>,
) -> ReviewReport {
    use super::super::install_types::PackageProvenance;
    use crate::{artifact_store::ArtifactStore, store::Store};
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let artifacts = ArtifactStore::open(root.path().join("artifacts"), [47; 32]).unwrap();
    if let Some(source) = source {
        std::fs::create_dir_all(root.path().join("artifacts.d/routes")).unwrap();
        std::fs::write(root.path().join("artifacts.d/routes/overlay.yaml"), source).unwrap();
    }
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra");
    let current =
        CapturedCurrentState::capture(&path, "lyra", root.path(), &store, &artifacts).unwrap();
    let overlay = super::super::review_overlay::assess(&current, candidate.registry()).unwrap();
    let identity = candidate.identity();
    let receipt = InstallReceipt {
        installation_id: ulid::Ulid::from(installation),
        package_id: identity.package_id,
        revision: identity.revision,
        inventory_format_version: identity.inventory_format_version,
        content_digest: identity.content_digest,
        manifest_digest: identity.manifest_digest,
        provenance: PackageProvenance::LocalUnverified,
        installed_at: "2026-01-01T00:00:00Z".into(),
        audit_id: ulid::Ulid::from(9_u128),
        audit_seq: 1,
    };
    let work = store
        .package_outstanding_work("2026-01-01T00:00:00Z".parse().unwrap())
        .unwrap();
    build(&current, candidate, receipt, overlay, work)
}

fn candidate() -> PackageSnapshot {
    super::super::inspect(
        &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra"),
    )
    .unwrap()
}

#[test]
fn report_is_deterministic_and_binds_the_exact_receipt_without_approval() {
    let candidate = candidate();
    let first = review(&candidate, 1);
    let again = review(&candidate, 1);
    assert_eq!(first.json(), again.json());
    let changed_receipt = review(&candidate, 2);
    assert_ne!(first.plan_digest, changed_receipt.plan_digest);
    let json: Value = serde_json::from_str(&first.json()).unwrap();
    assert_eq!(json["source"]["mode"], "legacy-configured");
    assert!(json["source"].get("installation_id").is_none());
    assert_eq!(json["approval"], "not-performed");
    assert_eq!(json["activation_supported"], false);
    assert!(first.summary().contains("untrusted data"));
}

#[test]
fn whole_report_bound_fails_closed_instead_of_hiding_required_detail() {
    use openspine_schemas::package::PackageDeclaration;
    let original = candidate();
    let mut files: BTreeMap<String, Vec<u8>> = original
        .files()
        .map(|(p, b)| (p.to_owned(), b.to_vec()))
        .collect();
    let mut declaration: PackageDeclaration =
        serde_yaml::from_slice(&files["package.yaml"]).unwrap();
    declaration.version += 1;
    let template = original.registry().policies.values().next().unwrap();
    for i in 0..36 {
        let mut policy = template.clone();
        policy.id = format!("large-review-{i:03}");
        policy.constraints.external_visibility_max = Some("x".repeat(60 * 1024));
        declaration.artifacts.policies.push(policy.id.clone());
        files.insert(
            format!("policies/{}.yaml", policy.id),
            serde_yaml::to_string(&policy).unwrap().into_bytes(),
        );
    }
    files.insert(
        "package.yaml".into(),
        serde_yaml::to_string(&declaration).unwrap().into_bytes(),
    );
    let candidate = super::super::PackageCapture::new(files).validate().unwrap();
    let report = review(&candidate, 1);
    assert!(report.json().len() < MAX_REPORT_BYTES);
    assert!(report.summary().len() < MAX_REPORT_BYTES);
    assert!(!report.plan.semantic_review_complete);
    assert!(report
        .plan
        .blockers
        .iter()
        .any(|code| code == "report-detail-limit-exceeded"));
    assert!(report.plan.omitted_details_digest.is_some());
    assert!(report.plan.base_semantics.is_none());
    assert_eq!(report.plan.approval, "not-performed");
}

#[test]
fn declaration_entry_agent_change_is_owner_visible_and_digest_bound() {
    use openspine_schemas::package::PackageDeclaration;
    let original = candidate();
    let mut files: BTreeMap<String, Vec<u8>> = original
        .files()
        .map(|(p, b)| (p.to_owned(), b.to_vec()))
        .collect();
    let mut declaration: PackageDeclaration =
        serde_yaml::from_slice(&files["package.yaml"]).unwrap();
    let old_agent = declaration.entry_agent.clone();
    let new_agent = original
        .registry()
        .agents
        .keys()
        .find(|id| **id != old_agent)
        .unwrap()
        .clone();
    declaration.entry_agent = new_agent.clone();
    files.insert(
        "package.yaml".into(),
        serde_yaml::to_string(&declaration).unwrap().into_bytes(),
    );
    let updated = super::super::PackageCapture::new(files).validate().unwrap();
    let before = review(&original, 1);
    let after = review(&updated, 1);
    let json: Value = serde_json::from_str(&after.json()).unwrap();
    assert_eq!(json["declarations"]["before"]["entry_agent"], old_agent);
    assert_eq!(json["declarations"]["after"]["entry_agent"], new_agent);
    assert_ne!(before.plan_digest, after.plan_digest);
    assert_ne!(
        before.plan.after_runtime_input_digest,
        after.plan.after_runtime_input_digest
    );
}

#[test]
fn captured_overlay_byte_changes_change_plan_and_runtime_input_digests() {
    let candidate = candidate();
    let one = "id: overlay\nschema_version: 1\nversion: 1\nlifecycle_state: active\npriority: 1\nwhen: {}\nagent: null\nworkflow: null\ncapability_pack: null\n";
    let two = one.replace("priority: 1", "priority: 2");
    let before = review_with_overlay(&candidate, 1, Some(one));
    let after = review_with_overlay(&candidate, 1, Some(&two));
    assert_ne!(before.plan_digest, after.plan_digest);
    assert_ne!(
        before.plan.before_runtime_input_digest,
        after.plan.before_runtime_input_digest
    );
    assert_eq!(before.plan.source.identity, after.plan.source.identity);
    assert_eq!(before.plan.candidate, after.plan.candidate);
}

#[path = "review_report_overlay_tests.rs"]
mod overlay_details;

#[test]
fn base_epoch_change_is_a_commit_consequence_not_a_prereconfirmation_blocker() {
    let original = candidate();
    let mut files: BTreeMap<String, Vec<u8>> = original
        .files()
        .map(|(p, b)| (p.to_owned(), b.to_vec()))
        .collect();
    let route = files
        .keys()
        .find(|path| path.starts_with("routes/"))
        .unwrap()
        .clone();
    files
        .get_mut(&route)
        .unwrap()
        .extend_from_slice(b"\n# changed exact base epoch\n");
    let updated = super::super::PackageCapture::new(files).validate().unwrap();
    let report = review(&updated, 1);
    assert!(
        report
            .plan
            .overlay
            .as_ref()
            .unwrap()
            .reusable_authority_reconfirmation_required
    );
    assert!(!report
        .plan
        .blockers
        .iter()
        .any(|code| code == "reusable-authority-reconfirmation-required"));
    let json: Value = serde_json::from_str(&report.json()).unwrap();
    assert_eq!(
        json["required_transition_consequences"][0],
        "invalidate-reusable-authority-and-require-normal-reconfirmation-before-reuse"
    );
}
