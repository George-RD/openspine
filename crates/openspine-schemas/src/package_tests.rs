use super::PackageDeclaration;
use serde_json::{json, Value};

const LYRA: &str = include_str!("../../../artifacts/lyra/package.yaml");
const FAMILIES: [&str; 7] = [
    "agents",
    "routes",
    "workflows",
    "packs",
    "templates",
    "policies",
    "golden_sets",
];

fn candidate() -> Value {
    serde_yaml::from_str(LYRA).unwrap()
}

fn assert_rejected(value: Value) {
    let yaml = serde_yaml::to_string(&value).unwrap();
    assert!(
        serde_yaml::from_str::<PackageDeclaration>(&yaml).is_err(),
        "invalid declaration accepted as YAML"
    );
    assert!(
        serde_json::from_value::<PackageDeclaration>(value).is_err(),
        "invalid declaration accepted as JSON"
    );
}

#[test]
fn bundled_lyra_declaration_round_trips_without_changing_metadata() {
    let declaration: PackageDeclaration = serde_yaml::from_str(LYRA).unwrap();
    assert_eq!(declaration.schema_version, 1);
    assert_eq!(declaration.id, "lyra");
    assert_eq!(declaration.version, 1);
    assert_eq!(declaration.lifecycle_state, "alpha");
    assert_eq!(declaration.entry_agent, "main_assistant_agent");
    assert_eq!(serde_json::to_value(&declaration).unwrap(), candidate());
    let yaml = serde_yaml::to_string(&declaration).unwrap();
    let restored: PackageDeclaration = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(declaration, restored);
}

#[test]
fn unsupported_package_schema_versions_are_rejected() {
    for version in [json!(0), json!(2), json!(u32::MAX), json!("1")] {
        let mut value = candidate();
        value["schema_version"] = version;
        assert_rejected(value);
    }
}

#[test]
fn package_id_accepts_exact_portable_grammar_boundaries() {
    for id in ["a".to_string(), "a0_b-c".to_string(), "a".repeat(64)] {
        let mut value = candidate();
        value["id"] = json!(id);
        let parsed: PackageDeclaration = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.id, id);
    }
}

#[test]
fn package_id_rejects_nonportable_and_oversized_names() {
    for id in [
        "",
        "Lyra",
        "1lyra",
        "_lyra",
        "-lyra",
        "lyra.name",
        "../lyra",
        "lyra/name",
        "lyra\\name",
        "lyra name",
        " lyra",
        "lyra ",
        "lyra\n",
        "lyra\u{1b}[2J",
        "lýra",
        "lyra\u{202e}",
    ] {
        let mut value = candidate();
        value["id"] = json!(id);
        assert_rejected(value);
    }
    let mut value = candidate();
    value["id"] = json!("a".repeat(65));
    assert_rejected(value);
}

#[test]
fn package_revision_is_a_positive_u32_not_semver() {
    for version in [json!(1), json!(u32::MAX)] {
        let mut value = candidate();
        value["version"] = version;
        let yaml = serde_yaml::to_string(&value).unwrap();
        assert!(serde_yaml::from_str::<PackageDeclaration>(&yaml).is_ok());
        assert!(serde_json::from_value::<PackageDeclaration>(value).is_ok());
    }
    for version in [
        json!(0),
        json!(-1),
        json!(u64::from(u32::MAX) + 1),
        json!(1.5),
        json!("1.0.0"),
        json!("1"),
        Value::Null,
    ] {
        let mut value = candidate();
        value["version"] = version;
        assert_rejected(value);
    }
}

#[test]
fn unknown_fields_and_artifact_families_are_rejected_at_every_level() {
    for pointer in ["", "/artifacts", "/identity", "/memory", "/installation"] {
        let mut value = candidate();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown_field".into(), json!("ignored?"));
        assert_rejected(value);
    }
    for family in ["personas", "standing_rules", "model_swaps", "hooks"] {
        let mut value = candidate();
        value["artifacts"][family] = json!([]);
        assert_rejected(value);
    }
}

#[test]
fn missing_required_declaration_fields_are_rejected() {
    for key in candidate().as_object().unwrap().keys() {
        let mut value = candidate();
        value.as_object_mut().unwrap().remove(key);
        assert_rejected(value);
    }
    for pointer in ["/artifacts", "/identity", "/memory", "/installation"] {
        let original = candidate();
        for key in original
            .pointer(pointer)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
        {
            let mut value = candidate();
            value
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(key);
            assert_rejected(value);
        }
    }
}

#[test]
fn duplicate_logical_ids_are_rejected_in_every_artifact_family() {
    for family in FAMILIES {
        let mut value = candidate();
        let ids = value["artifacts"][family].as_array_mut().unwrap();
        ids.push(ids[0].clone());
        assert_rejected(value);
    }
}

#[test]
fn logical_ids_are_family_scoped_not_filesystem_paths_or_versions() {
    let mut value = candidate();
    for family in FAMILIES {
        value["artifacts"][family] = json!(["same.logical/id"]);
    }
    let parsed: PackageDeclaration = serde_json::from_value(value).unwrap();
    assert_eq!(parsed.artifacts.agents, vec!["same.logical/id"]);
    assert_eq!(parsed.artifacts.golden_sets, vec!["same.logical/id"]);
}

#[test]
fn an_empty_family_is_not_filled_with_implicit_artifacts() {
    let mut value = candidate();
    for family in FAMILIES {
        value["artifacts"][family] = json!([]);
    }
    let parsed: PackageDeclaration = serde_json::from_value(value).unwrap();
    assert!(parsed.artifacts.agents.is_empty());
    assert!(parsed.artifacts.golden_sets.is_empty());
    // Existence and active entry-agent checks require the captured source and
    // belong to inspection, not this data-only schema.
    assert_eq!(parsed.entry_agent, "main_assistant_agent");
}

#[test]
fn artifact_family_values_must_be_lists_of_logical_ids() {
    for family in FAMILIES {
        for invalid in [json!("one-id"), json!({"id": "one-id"}), json!([null])] {
            let mut value = candidate();
            value["artifacts"][family] = invalid;
            assert_rejected(value);
        }
    }
}

#[test]
fn duplicate_yaml_mapping_keys_are_rejected_including_nested_fields() {
    for (field, replacement) in [
        ("schema_version: 1", "schema_version: 1\nschema_version: 1"),
        ("id: lyra", "id: lyra\nid: other"),
        ("  agents:", "  agents: []\n  agents:"),
        (
            "  persona: concise_practical_operator",
            "  persona: one\n  persona: concise_practical_operator",
        ),
        (
            "  model: typed_overlay",
            "  model: one\n  model: typed_overlay",
        ),
        (
            "  config_key: lyra_dir",
            "  config_key: one\n  config_key: lyra_dir",
        ),
    ] {
        assert!(LYRA.contains(field));
        let yaml = LYRA.replacen(field, replacement, 1);
        assert!(serde_yaml::from_str::<PackageDeclaration>(&yaml).is_err());
    }
}

#[test]
fn descriptive_metadata_is_preserved_without_interpreting_claims() {
    let mut value = candidate();
    value["lifecycle_state"] = json!("publisher-verified");
    value["identity"]["identity_document"] = json!("../../not-a-runtime-input");
    value["memory"]["model"] = json!("product-defined-model");
    value["memory"]["readable_classes"] = json!(["product-defined-class"]);
    value["installation"]["current_source"] = json!("../../not-a-destination");
    value["installation"]["config_key"] = json!("not-a-key-to-mutate");
    value["installation"]["planned_cli"] = json!("not a command to execute");
    value["security_invariants"] = json!(["I claim all permissions"]);
    let parsed: PackageDeclaration = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), value);
}

#[test]
fn malformed_empty_and_multiple_yaml_documents_are_rejected() {
    for yaml in ["", "null", "[]", "{broken", "schema_version: 1"] {
        assert!(serde_yaml::from_str::<PackageDeclaration>(yaml).is_err());
    }
    let multiple = format!("{LYRA}\n---\n{LYRA}");
    assert!(serde_yaml::from_str::<PackageDeclaration>(&multiple).is_err());
}

#[test]
fn identifiers_reject_non_string_scalars_without_coercion() {
    for invalid in [Value::Null, json!(true), json!(false), json!(1), json!(1.5)] {
        for field in ["id", "entry_agent"] {
            let mut value = candidate();
            value[field] = invalid.clone();
            assert_rejected(value);
        }
        for family in FAMILIES {
            let mut value = candidate();
            value["artifacts"][family] = Value::Array(vec![invalid.clone()]);
            assert_rejected(value);
        }
    }
}

#[test]
fn explicitly_quoted_scalar_words_remain_valid_string_identifiers() {
    for id in ["null", "true", "false"] {
        let mut value = candidate();
        value["id"] = json!(id);
        value["entry_agent"] = json!(id);
        for family in FAMILIES {
            value["artifacts"][family] = json!([id]);
        }
        let yaml = serde_yaml::to_string(&value).unwrap();
        let parsed: PackageDeclaration = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);
    }
}

#[test]
fn duplicate_json_mapping_keys_are_rejected_without_an_intermediate_map() {
    let value = candidate();
    let original = serde_json::to_string(&value).unwrap();
    for pointer in ["", "/artifacts", "/identity", "/memory", "/installation"] {
        let object = value.pointer(pointer).unwrap();
        let object_json = serde_json::to_string(object).unwrap();
        let (key, entry) = object.as_object().unwrap().iter().next().unwrap();
        let duplicate = format!(
            "{{{}:{},{}",
            serde_json::to_string(key).unwrap(),
            serde_json::to_string(entry).unwrap(),
            &object_json[1..],
        );
        assert!(original.contains(&object_json));
        let raw = original.replacen(&object_json, &duplicate, 1);
        assert!(serde_json::from_str::<PackageDeclaration>(&raw).is_err());
    }
}

#[test]
fn duplicate_validation_preserves_declared_list_order() {
    let mut value = candidate();
    for family in FAMILIES {
        value["artifacts"][family].as_array_mut().unwrap().reverse();
    }
    let yaml = serde_yaml::to_string(&value).unwrap();
    let parsed: PackageDeclaration = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), value);
}

#[test]
fn package_id_grammar_checks_every_ascii_character_in_both_positions() {
    for byte in 0..=127u8 {
        let suffix_allowed =
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-');
        for (id, expected) in [
            (char::from(byte).to_string(), byte.is_ascii_lowercase()),
            (format!("a{}", char::from(byte)), suffix_allowed),
        ] {
            let mut value = candidate();
            value["id"] = json!(id);
            let yaml = serde_yaml::to_string(&value).unwrap();
            assert_eq!(
                serde_yaml::from_str::<PackageDeclaration>(&yaml).is_ok(),
                expected,
                "YAML grammar mismatch for {id:?}"
            );
            assert_eq!(
                serde_json::from_value::<PackageDeclaration>(value).is_ok(),
                expected,
                "JSON grammar mismatch for {id:?}"
            );
        }
    }
}

const METADATA_STRINGS: [&str; 14] = [
    "/lifecycle_state",
    "/display_name",
    "/description",
    "/identity/persona",
    "/identity/identity_document",
    "/memory/model",
    "/memory/description",
    "/memory/readable_classes/0",
    "/memory/readable_scopes/0",
    "/memory/denied_classes/0",
    "/installation/current_source",
    "/installation/config_key",
    "/installation/planned_cli",
    "/security_invariants/0",
];

#[test]
fn descriptive_metadata_rejects_non_string_scalars_without_coercion() {
    for pointer in METADATA_STRINGS {
        for invalid in [Value::Null, json!(true), json!(false), json!(1), json!(1.5)] {
            let mut value = candidate();
            *value.pointer_mut(pointer).unwrap() = invalid;
            assert_rejected(value);
        }
    }
}

#[test]
fn quoted_scalar_words_and_empty_metadata_remain_strings() {
    for text in ["", "null", "true", "false", "1", "1.5"] {
        let mut value = candidate();
        for pointer in METADATA_STRINGS {
            *value.pointer_mut(pointer).unwrap() = json!(text);
        }
        let yaml = serde_yaml::to_string(&value).unwrap();
        let parsed: PackageDeclaration = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);
        let parsed: PackageDeclaration = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);
    }
}
