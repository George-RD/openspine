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
    assert!(registry.golden_sets.contains_key("model_swap_default"));

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
fn kind_table_round_trips_all_six_kinds() {
    let base = repo_lyra_dir();
    let fixtures = [
        ("route", "routes/owner_telegram_main_assistant.yaml"),
        ("agent", "agents/main_assistant_agent.yaml"),
        ("workflow", "workflows/owner_control_conversation.yaml"),
        ("pack", "packs/owner_control_basic_pack.yaml"),
        ("policy", "policies/global.yaml"),
    ];
    for spec in ARTIFACT_KIND_SPECS {
        let owned_yaml;
        let yaml = if spec.name == "model_swap" {
            owned_yaml = "id: base\nversion: 1\nlifecycle_state: proposed\nrole: base\ntarget_provider_id: test-provider\ngolden_set_id: model_swap_default\ngolden_set_result: null\n".to_string();
            owned_yaml.as_str()
        } else {
            let (_, rel) = fixtures
                .iter()
                .find(|(name, _)| *name == spec.name)
                .unwrap_or_else(|| panic!("test fixture map must cover kind {}", spec.name));
            owned_yaml = std::fs::read_to_string(base.join(rel))
                .unwrap_or_else(|e| panic!("fixture {rel} must exist: {e}"));
            owned_yaml.as_str()
        };
        let parsed =
            (spec.parse)(yaml).unwrap_or_else(|e| panic!("fixture {} must parse: {e}", spec.name));
        assert_eq!(parsed.kind(), spec.name);
        assert_eq!(parsed.overlay_subdir(), spec.overlay_subdir);
        let via_public = parse_proposal(spec.name, yaml)
            .unwrap_or_else(|e| panic!("parse_proposal must agree with table: {e}"));
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

/// AD-080: personas carry no authority. Unlike `template` (fixture-only,
/// never an artifact kind at all), `persona` IS a real, loaded artifact
/// kind — it just never appears in the proposable-kind table, so it can
/// never enter the propose -> approve -> activate pipeline.
#[test]
fn kind_table_excludes_personas() {
    assert!(find_kind_spec("persona").is_none());
    assert!(!is_proposable_kind("persona"));
    let err = parse_proposal("persona", "id: x\nschema_version: 1\n").unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("unknown artifact kind persona"),
        "unexpected parse error for persona: {msg}"
    );
}

#[test]
fn generic_overlay_loader_excludes_persona_and_base_loader_rejects_fixture() {
    let dir = tempfile::tempdir().unwrap();
    let personas_dir = dir.path().join("personas");
    std::fs::create_dir_all(&personas_dir).unwrap();
    std::fs::write(
        personas_dir.join("seed.yaml"),
        "id: seed\nschema_version: 1\nversion: 1\nlifecycle_state: active\nguidance: positive guidance\n",
    )
    .unwrap();

    let overlay_registry = load_registry(dir.path()).unwrap();
    assert!(overlay_registry.personas.is_empty());
    let err = load_base_registry(dir.path()).unwrap_err();
    assert!(matches!(
        err,
        ArtifactLoadError::Invalid { kind, .. } if kind == "persona"
    ));
}

#[test]
fn orphan_higher_persona_cannot_hide_row_backed_lower_version() {
    let dir = tempfile::tempdir().unwrap();
    let personas_dir = dir.path().join("personas");
    std::fs::create_dir_all(&personas_dir).unwrap();
    let v1 = PersonaElement {
        id: "seed".into(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        guidance: "first".into(),
    };
    let v2 = PersonaElement {
        id: "seed".into(),
        schema_version: 1,
        version: 2,
        lifecycle_state: Lifecycle::Active,
        guidance: "orphan".into(),
    };
    let v1_yaml = serde_yaml::to_string(&v1).unwrap();
    std::fs::write(personas_dir.join("v1.yaml"), &v1_yaml).unwrap();
    std::fs::write(
        personas_dir.join("v2.yaml"),
        serde_yaml::to_string(&v2).unwrap(),
    )
    .unwrap();

    std::fs::write(personas_dir.join("malformed.yaml"), "not: [valid").unwrap();
    let mut registry = load_registry_without_personas(dir.path()).unwrap();
    let admitted = std::collections::HashMap::from([(
        ("seed".to_string(), 1),
        openspine_schemas::digest::digest_of_bytes(v1_yaml.as_bytes()).to_string(),
    )]);
    load_admitted_personas(&mut registry, dir.path(), &admitted).unwrap();

    assert_eq!(registry.personas.get("seed").unwrap().version, 1);
    assert!(registry
        .sources
        .contains_key(&("persona".into(), "seed".into(), 1)));
    assert!(!registry
        .sources
        .contains_key(&("persona".into(), "seed".into(), 2)));
}
