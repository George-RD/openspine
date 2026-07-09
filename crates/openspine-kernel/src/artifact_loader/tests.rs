//! Tests for `artifact_loader`, extracted from the inline `mod tests` so the
//! source file stays under the 500-line budget (ROLE rule). Keeps the four
//! registry-loading tests and adds the kind-table round-trip / template-exclusion
//! tests mandated by `refactor-kernel-registries` (kernel-readiness item 1).

use super::*;

fn repo_lyra_dir() -> std::path::PathBuf {
    // crates/openspine-kernel -> repo root -> artifacts/lyra
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra")
}

#[test]
fn loads_every_real_fixture_without_error() {
    let registry = load_registry(&repo_lyra_dir()).expect("real fixtures must all parse");
    assert!(!registry.routes.is_empty());
    assert!(registry.agents.contains_key("main_assistant_agent"));
    assert!(registry
        .workflows
        .contains_key("owner_control_conversation"));
    assert!(registry.packs.contains_key("owner_control_basic_pack"));
    assert!(registry.policies.contains_key("global"));
    assert!(registry.templates.contains_key("owner_control_template"));

    // Step 5 (implement-selected-thread-email-preview-slice) fixtures.
    assert!(registry.agents.contains_key("email_reply_drafter"));
    assert!(registry
        .workflows
        .contains_key("selected_thread_email_reply_draft"));
    assert!(registry
        .packs
        .contains_key("selected_thread_email_draft_pack"));
    assert!(registry
        .routes
        .iter()
        .any(|r| r.id == "owner_email_selected_thread"));
    assert!(registry
        .templates
        .contains_key("email_reply_draft_template"));
}

#[test]
fn missing_directory_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let registry = load_registry(dir.path()).expect("no subdirectories at all is fine");
    assert!(registry.routes.is_empty());
    assert!(registry.agents.is_empty());
}

#[test]
fn malformed_fixture_fails_to_load() {
    let dir = tempfile::tempdir().unwrap();
    let agents_dir = dir.path().join("agents");
    std::fs::create_dir_all(&agents_dir).unwrap();
    std::fs::write(agents_dir.join("bad.yaml"), "id: x\nunknown_field: true\n").unwrap();
    let err = load_registry(dir.path()).unwrap_err();
    assert!(matches!(err, ArtifactLoadError::Parse { .. }));
}

#[test]
fn non_yaml_files_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let routes_dir = dir.path().join("routes");
    std::fs::create_dir_all(&routes_dir).unwrap();
    std::fs::write(routes_dir.join("README.md"), "not yaml").unwrap();
    let registry = load_registry(dir.path()).expect("non-yaml files must be skipped");
    assert!(registry.routes.is_empty());
}

/// The kind table is the single source of truth: every entry's `parse` must
/// round-trip a real fixture into a [`ParsedProposal`] whose `kind()` and
/// `overlay_subdir()` agree with the table entry — and the public
/// `parse_proposal` entry point must agree too (D-048).
#[test]
fn kind_table_round_trips_all_five_kinds() {
    let base = repo_lyra_dir();
    let fixtures = [
        ("route", "routes/owner_telegram_main_assistant.yaml"),
        ("agent", "agents/main_assistant_agent.yaml"),
        ("workflow", "workflows/owner_control_conversation.yaml"),
        ("pack", "packs/owner_control_basic_pack.yaml"),
        ("policy", "policies/global.yaml"),
    ];
    for spec in ARTIFACT_KIND_SPECS {
        let (expected_name, rel) = fixtures
            .iter()
            .find(|(name, _)| *name == spec.name)
            .unwrap_or_else(|| panic!("test fixture map must cover kind {}", spec.name));
        let yaml = std::fs::read_to_string(base.join(rel))
            .unwrap_or_else(|e| panic!("fixture {rel} must exist: {e}"));
        // Parse through the table entry (the single source of truth).
        let parsed = (spec.parse)(yaml.as_str())
            .unwrap_or_else(|e| panic!("fixture {rel} must parse via its table entry: {e}"));
        assert_eq!(parsed.kind(), *expected_name);
        // overlay_subdir() must agree with the table (D-048 kind table).
        assert_eq!(parsed.overlay_subdir(), spec.overlay_subdir);
        // The public entry point must agree with the table too.
        let via_public = parse_proposal(spec.name, yaml.as_str())
            .unwrap_or_else(|e| panic!("parse_proposal must agree with the table: {e}"));
        assert_eq!(via_public.kind(), spec.name);
        assert_eq!(via_public.overlay_subdir(), spec.overlay_subdir);
    }
}

/// D-048: templates MUST NOT appear in the proposable-kind table, and parsing
/// one must fail fast exactly like any other unknown kind.
#[test]
fn kind_table_excludes_templates() {
    assert!(find_kind_spec("template").is_none());
    assert!(!is_proposable_kind("template"));
    let err = parse_proposal("template", "id: x\nschema_version: 1\n").unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("unknown artifact kind template"),
        "unexpected parse error for template: {msg}"
    );
}
