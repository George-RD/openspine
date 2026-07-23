// openspine:allow-large-module reason: cohesive overlay compatibility fixed-point and registry-exclusion matrix shares artifact fixtures across every registered kind.
use super::*;
use crate::store::learned_artifacts::{dependency_fingerprint, NominationStatus, Provenance};
use jiff::Timestamp;
use openspine_schemas::artifact::{ArtifactNamespace, ArtifactRef, Lifecycle};
use openspine_schemas::digest::digest_of_bytes;
use openspine_schemas::route::{Route, RouteEffect};
use std::collections::HashSet;
use ulid::Ulid;

fn learned(kind: &str, id: &str) -> LearnedArtifact {
    LearnedArtifact {
        kind: kind.into(),
        artifact_id: id.into(),
        version: 1,
        namespace: ArtifactNamespace::Overlay,
        provenance: Provenance::ProducedBy {
            source_event_id: Ulid::new(),
            source_exchange: ArtifactRef {
                digest: openspine_schemas::digest::digest_of_bytes(b"exchange"),
                schema_version: 1,
            },
            source_scope: crate::counterparty_keys::SYSTEM_SCOPE,
        },
        accepted_via: None,
        learned_at: Timestamp::now(),
        compatibility: CompatibilityStatus::Compatible,
        nomination: NominationStatus::None,
        pending_reconfirmation_id: None,
        pending_yaml_digest: None,
        accepted_dependency_fingerprint: None,
        source_path: None,
        accepted_base_epoch: None,
    }
}

#[test]
fn dangling_learned_route_is_orphaned_and_excluded() {
    let mut registry = ArtifactRegistry::default();
    registry.routes.push(Route {
        id: "learned-route".into(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        priority: None,
        effect: RouteEffect::Allow,
        when: Default::default(),
        agent: Some("removed-agent".into()),
        workflow: None,
        capability_pack: None,
        persona: None,
    });
    let (orphans, requests) =
        apply_compatibility(&mut registry, &[learned("route", "learned-route")]);
    assert_eq!(orphans[0].dangling_references, vec!["agent:removed-agent"]);
    assert_eq!(requests.len(), 1);
    assert!(registry.routes.is_empty());
}

#[test]
fn reconfirm_request_reuses_unchanged_and_rotates_changed_payload() {
    let store = crate::store::Store::open_in_memory().unwrap();
    let request_id = Ulid::new();
    let first_ref = ArtifactRef {
        digest: openspine_schemas::digest::digest_of_bytes(b"yaml-v1"),
        schema_version: 1,
    };
    let reused =
        ensure_reconfirm_request(&store, "route", "route", 1, request_id, first_ref.clone())
            .unwrap();
    assert_eq!(reused, request_id);
    assert_eq!(
        ensure_reconfirm_request(&store, "route", "route", 1, request_id, first_ref).unwrap(),
        request_id
    );
    let rotated = ensure_reconfirm_request(
        &store,
        "route",
        "route",
        1,
        request_id,
        ArtifactRef {
            digest: openspine_schemas::digest::digest_of_bytes(b"yaml-v2"),
            schema_version: 1,
        },
    )
    .unwrap();
    assert_ne!(rotated, request_id);
}

#[test]
fn base_namespace_is_not_treated_as_learned() {
    let registry = ArtifactRegistry::default();
    let mut artifact = learned("route", "route");
    artifact.namespace = ArtifactNamespace::Base;
    assert!(find_orphans(&registry, &[artifact]).is_empty());
}

#[test]
fn owner_accepted_route_with_no_refs_is_compatible_after_epoch_change() {
    // Unrelated base change: a self-contained route (no agent/workflow/pack
    // refs) has no typed dependency to revalidate, so revalidation yields no
    // newly-dangling references — the overlay stays accepted and its epoch
    // is refreshed silently.
    let yaml = "id: r\nschema_version: 1\nversion: 1\nlifecycle_state: active\n\
                effect: allow\n";
    let registry = ArtifactRegistry::default();
    let newly = owner_accepted_newly_dangling(&registry, "route", Some(yaml.as_bytes()));
    assert!(
        newly.is_empty(),
        "self-contained route must be compatible, got {newly:?}"
    );
}

#[test]
fn owner_accepted_route_with_removed_ref_is_newly_dangling() {
    // A referenced base agent that is no longer active is a newly-dangling
    // typed reference — the overlay must be excluded and re-prompted.
    let yaml = "id: r\nschema_version: 1\nversion: 1\nlifecycle_state: active\n\
                effect: allow\nagent: removed-agent\n";
    let registry = ArtifactRegistry::default();
    let newly = owner_accepted_newly_dangling(&registry, "route", Some(yaml.as_bytes()));
    assert_eq!(newly, vec!["agent:removed-agent"]);
}

#[test]
fn owner_accepted_missing_source_is_fail_closed() {
    let registry = ArtifactRegistry::default();
    let newly = owner_accepted_newly_dangling(&registry, "route", None);
    assert_eq!(newly, vec!["owner_accepted_source_missing"]);
}

const MAIN_AGENT: &str = include_str!("../../../artifacts/lyra/agents/main_assistant_agent.yaml");
const OWNER_PACK: &str =
    include_str!("../../../artifacts/lyra/packs/owner_control_basic_pack.yaml");

fn route_yaml(id: &str, agent: Option<&str>, workflow: Option<&str>) -> String {
    let mut body = format!(
        "id: {id}\nschema_version: 1\nversion: 1\nlifecycle_state: active\neffect: allow\n"
    );
    if let Some(agent) = agent {
        body.push_str(&format!("agent: {agent}\n"));
    }
    if let Some(workflow) = workflow {
        body.push_str(&format!("workflow: {workflow}\n"));
    }
    body
}

fn workflow_yaml(id: &str, agent: &str, pack: &str) -> String {
    format!(
        "id: {id}\nschema_version: 1\nversion: 1\nlifecycle_state: active\npurpose: p\n\
         required_agent: {agent}\nrequired_capability_pack: {pack}\n"
    )
}

fn write_yaml(name: &str, yaml: &str) -> (std::path::PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("overlay_om_{}_{}", name, ulid::Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.yaml"));
    std::fs::write(&path, yaml.as_bytes()).unwrap();
    let digest = digest_of_bytes(yaml.as_bytes()).to_string();
    (path, digest)
}

fn insert_agent(registry: &mut ArtifactRegistry, yaml: &str) {
    let agent: openspine_schemas::agent::AgentManifest = serde_yaml::from_str(yaml).unwrap();
    registry.agents.insert(agent.id.clone(), agent);
}

fn insert_pack(registry: &mut ArtifactRegistry, yaml: &str) {
    let pack: openspine_schemas::pack::CapabilityPack = serde_yaml::from_str(yaml).unwrap();
    registry.packs.insert(pack.id.clone(), pack);
}

fn insert_workflow(registry: &mut ArtifactRegistry, yaml: &str) {
    let workflow: openspine_schemas::workflow::WorkflowManifest =
        serde_yaml::from_str(yaml).unwrap();
    registry.workflows.insert(workflow.id.clone(), workflow);
}

fn insert_route(registry: &mut ArtifactRegistry, yaml: &str) {
    let mut parsed = crate::artifact_loader::parse_proposal("route", yaml).unwrap();
    parsed.activate();
    parsed.insert_into(registry).unwrap();
}

#[test]
fn unchanged_accepted_dangling_survives_restart() {
    // A pre-existing owner-accepted dangling reference (agent gone) must
    // survive an unrelated restart: the durable fingerprint covers it, so the
    // overlay is not re-prompted and stays in the effective registry.
    let yaml = route_yaml("kept-route", Some("removed-agent"), None);
    let (path, digest) = write_yaml("kept-route", &yaml);
    let mut registry = ArtifactRegistry::default();
    insert_route(&mut registry, &yaml);
    let mut accepted = learned("route", "kept-route");
    accepted.compatibility = CompatibilityStatus::OwnerAccepted;
    accepted.source_path = Some(path.to_string_lossy().into_owned());
    accepted.pending_yaml_digest = Some(digest);
    accepted.accepted_dependency_fingerprint =
        Some(dependency_fingerprint(&["agent:removed-agent".to_string()]));
    let (ordinary, _req, invalid) =
        converge_owner_accepted_dependencies(&mut registry, &[accepted], &HashSet::new(), &[]);
    assert!(
        invalid.is_empty(),
        "accepted dangling must survive restart: {invalid:?}"
    );
    assert!(ordinary.is_empty());
    assert!(registry.routes.iter().any(|route| route.id == "kept-route"));
}

#[test]
fn accepted_digest_tamper_invalidates() {
    // The reviewed YAML on disk no longer matches the recorded digest: tampered
    // reviewed bytes must never become effective and must be invalidated.
    let original = route_yaml("tamper-route", Some("removed-agent"), None);
    let (path, digest) = write_yaml("tamper-route", &original);
    let tampered = route_yaml("tamper-route", Some("other-removed"), None);
    std::fs::write(&path, tampered.as_bytes()).unwrap();
    let mut registry = ArtifactRegistry::default();
    insert_route(&mut registry, &tampered);
    let mut accepted = learned("route", "tamper-route");
    accepted.compatibility = CompatibilityStatus::OwnerAccepted;
    accepted.source_path = Some(path.to_string_lossy().into_owned());
    accepted.pending_yaml_digest = Some(digest);
    accepted.accepted_dependency_fingerprint =
        Some(dependency_fingerprint(&["agent:removed-agent".to_string()]));
    let (_ordinary, _req, invalid) =
        converge_owner_accepted_dependencies(&mut registry, &[accepted], &HashSet::new(), &[]);
    assert_eq!(invalid.len(), 1, "{invalid:?}");
    assert_eq!(
        invalid[0].dangling_references,
        vec!["owner_accepted_digest_tampered"]
    );
}

#[test]
fn owner_accepted_route_invalidated_when_ordinary_agent_excluded() {
    // OwnerAccepted A depends on an ordinary B (agent). When the ordinary
    // dependency is excluded by the ordinary pass, the owner-accepted pass
    // must revalidate and invalidate A (newly-dangling reference).
    let yaml = route_yaml("a-route", Some("main_assistant_agent"), None);
    let (path, digest) = write_yaml("a-route", &yaml);
    let mut registry = ArtifactRegistry::default();
    insert_agent(&mut registry, MAIN_AGENT);
    insert_route(&mut registry, &yaml);
    let mut b = learned("agent", "main_assistant_agent");
    b.compatibility = CompatibilityStatus::ReconfirmationRequired;
    let mut a = learned("route", "a-route");
    a.compatibility = CompatibilityStatus::OwnerAccepted;
    a.source_path = Some(path.to_string_lossy().into_owned());
    a.pending_yaml_digest = Some(digest);
    a.accepted_dependency_fingerprint = Some(dependency_fingerprint(&[]));
    let (ordinary, _req, invalid) =
        converge_owner_accepted_dependencies(&mut registry, &[b, a], &HashSet::new(), &[]);
    assert!(
        ordinary
            .iter()
            .any(|orphan| orphan.artifact_id == "main_assistant_agent"),
        "ordinary B excluded: {ordinary:?}"
    );
    assert_eq!(invalid.len(), 1, "{invalid:?}");
    assert_eq!(invalid[0].artifact_id, "a-route");
    assert_eq!(
        invalid[0].dangling_references,
        vec!["agent:main_assistant_agent"]
    );
}

#[test]
fn same_version_base_collision_owner_accepted_survives() {
    // A base/overlay (kind,id,version) collision must not be stripped by the
    // owner-accepted convergence, and its accepted (empty) dependency set is
    // unaffected by an unrelated active base change.
    let yaml = route_yaml("col-route", Some("main_assistant_agent"), None);
    let (path, digest) = write_yaml("col-route", &yaml);
    let mut registry = ArtifactRegistry::default();
    insert_agent(&mut registry, MAIN_AGENT);
    insert_route(&mut registry, &yaml);
    let base_ids = HashSet::from([("route".to_string(), "col-route".to_string())]);
    let mut accepted = learned("route", "col-route");
    accepted.compatibility = CompatibilityStatus::OwnerAccepted;
    accepted.source_path = Some(path.to_string_lossy().into_owned());
    accepted.pending_yaml_digest = Some(digest);
    accepted.accepted_dependency_fingerprint = Some(dependency_fingerprint(&[]));
    let (ordinary, _req, invalid) =
        converge_owner_accepted_dependencies(&mut registry, &[accepted], &base_ids, &[]);
    assert!(
        invalid.is_empty(),
        "base-collision owner-accepted must survive: {invalid:?}"
    );
    assert!(ordinary.is_empty());
    assert!(registry.routes.iter().any(|route| route.id == "col-route"));
}

#[test]
fn transitive_chain_owner_accepted_invalidated_to_fixed_point() {
    // ordinary trigger-agent excluded -> owner-accepted workflow B (its
    // required_agent) newly dangling -> B excluded -> ordinary route A
    // referencing B dangling -> A excluded. The alternating loop must reach
    // this fixed point: both ordinary A and owner-accepted B are handled.
    let wf_yaml = workflow_yaml("b-wf", "main_assistant_agent", "owner_control_basic_pack");
    let (wf_path, wf_digest) = write_yaml("b-wf", &wf_yaml);
    let a_yaml = route_yaml("a-route", None, Some("b-wf"));
    let (a_path, a_digest) = write_yaml("a-route", &a_yaml);
    let mut registry = ArtifactRegistry::default();
    insert_agent(&mut registry, MAIN_AGENT);
    insert_pack(&mut registry, OWNER_PACK);
    insert_workflow(&mut registry, &wf_yaml);
    insert_route(&mut registry, &a_yaml);
    let mut trigger = learned("agent", "main_assistant_agent");
    trigger.compatibility = CompatibilityStatus::ReconfirmationRequired;
    let mut b = learned("workflow", "b-wf");
    b.compatibility = CompatibilityStatus::OwnerAccepted;
    b.source_path = Some(wf_path.to_string_lossy().into_owned());
    b.pending_yaml_digest = Some(wf_digest);
    b.accepted_dependency_fingerprint = Some(dependency_fingerprint(&[]));
    let mut a = learned("route", "a-route");
    a.compatibility = CompatibilityStatus::Compatible;
    a.source_path = Some(a_path.to_string_lossy().into_owned());
    a.pending_yaml_digest = Some(a_digest);
    let (ordinary, _req, invalid) =
        converge_owner_accepted_dependencies(&mut registry, &[trigger, b, a], &HashSet::new(), &[]);
    assert!(
        ordinary
            .iter()
            .any(|orphan| orphan.artifact_id == "main_assistant_agent"),
        "trigger excluded: {ordinary:?}"
    );
    assert!(
        ordinary
            .iter()
            .any(|orphan| orphan.artifact_id == "a-route"),
        "ordinary A excluded: {ordinary:?}"
    );
    assert!(
        invalid.iter().any(|orphan| orphan.artifact_id == "b-wf"),
        "owner-accepted B invalidated: {invalid:?}"
    );
    assert_eq!(invalid.len(), 1);
}
fn insert_source(registry: &mut ArtifactRegistry, kind: &str, id: &str, version: u32) {
    registry.sources.insert(
        (kind.into(), id.into(), version),
        crate::artifact_loader::ArtifactSource {
            path: std::path::PathBuf::from(format!("/tmp/{kind}-{id}-v{version}.yaml")),
            bytes: format!("{kind}:{id}:v{version}").into_bytes(),
        },
    );
}

fn erased(kind: &str, id: &str, version: u32) -> LearnedArtifact {
    let mut item = learned(kind, id);
    item.version = version;
    item.compatibility = CompatibilityStatus::Erased;
    item
}

/// Every learnable kind that can land in the live registry and must honor
/// exact-version erase exclusion at startup (proposable ARTIFACT_KIND_SPECS
/// plus template/persona overlays).
const ERASE_EXCLUSION_KINDS: &[&str] = &[
    "route",
    "agent",
    "workflow",
    "pack",
    "policy",
    "model_swap",
    "standing_rule",
    "template",
    "persona",
];

#[test]
fn exclude_erased_covers_registered_kinds_and_exact_version_model_swaps() {
    use crate::artifact_loader::ARTIFACT_KIND_SPECS;
    use crate::model_gateway::PromptTemplate;
    use openspine_schemas::action::ActionId;
    use openspine_schemas::model_swap::{ModelRole, ModelSwapManifest};
    use openspine_schemas::persona::PersonaElement;
    use openspine_schemas::standing_rule::{BudgetWindow, StandingRuleManifest};

    // Guard: every proposable kind is covered by the erase matrix.
    for spec in ARTIFACT_KIND_SPECS {
        assert!(
            ERASE_EXCLUSION_KINDS.contains(&spec.name),
            "registered kind {} missing from erase exclusion coverage",
            spec.name
        );
    }

    let mut registry = ArtifactRegistry::default();

    // route (erased v1)
    registry.routes.push(Route {
        id: "erased-route".into(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        priority: None,
        effect: RouteEffect::Allow,
        when: Default::default(),
        agent: None,
        workflow: None,
        capability_pack: None,
        persona: None,
    });
    insert_source(&mut registry, "route", "erased-route", 1);

    // agent/workflow/pack from fixture YAML, remapped to erased ids.
    let mut agent: openspine_schemas::agent::AgentManifest =
        serde_yaml::from_str(MAIN_AGENT).unwrap();
    agent.id = "erased-agent".into();
    agent.version = 1;
    registry.agents.insert(agent.id.clone(), agent);
    insert_source(&mut registry, "agent", "erased-agent", 1);

    let mut pack: openspine_schemas::pack::CapabilityPack =
        serde_yaml::from_str(OWNER_PACK).unwrap();
    pack.id = "erased-pack".into();
    pack.version = 1;
    registry.packs.insert(pack.id.clone(), pack);
    insert_source(&mut registry, "pack", "erased-pack", 1);

    let workflow_yaml = workflow_yaml("erased-workflow", "erased-agent", "erased-pack");
    insert_workflow(&mut registry, &workflow_yaml);
    insert_source(&mut registry, "workflow", "erased-workflow", 1);

    let mut policy: openspine_schemas::policy::Policy =
        serde_yaml::from_str(include_str!("../../../artifacts/lyra/policies/global.yaml")).unwrap();
    policy.id = "erased-policy".into();
    policy.version = 1;
    registry.policies.insert(policy.id.clone(), policy);
    insert_source(&mut registry, "policy", "erased-policy", 1);

    // model_swap: erased v1 must leave, same-id v2 (live) must stay.
    // HashMap keeps one entry per id, so the live map holds v2 while the
    // erased learned row names v1 — exact-version match refuses removal.
    registry.model_swaps.insert(
        "base".into(),
        ModelSwapManifest {
            id: "base".into(),
            version: 2,
            lifecycle_state: Lifecycle::Active,
            role: ModelRole::Base,
            target_provider_id: "provider-live".into(),
            golden_set_id: "model_swap_default".into(),
            golden_set_result: None,
        },
    );
    insert_source(&mut registry, "model_swap", "base", 1);
    insert_source(&mut registry, "model_swap", "base", 2);

    // Separate erased model_swap identity that is present at matching version.
    registry.model_swaps.insert(
        "matcher".into(),
        ModelSwapManifest {
            id: "matcher".into(),
            version: 1,
            lifecycle_state: Lifecycle::Active,
            role: ModelRole::Matcher,
            target_provider_id: "provider-erased".into(),
            golden_set_id: "model_swap_default".into(),
            golden_set_result: None,
        },
    );
    insert_source(&mut registry, "model_swap", "matcher", 1);

    registry.standing_rules.insert(
        "erased-rule".into(),
        StandingRuleManifest {
            id: "erased-rule".into(),
            schema_version: 1,
            version: 1,
            lifecycle_state: Lifecycle::Active,
            action_id: ActionId::new("calendar.book_appointment"),
            description: "erased standing rule".into(),
            quota: BudgetWindow {
                max: 5,
                window_secs: 604_800,
            },
            rate: BudgetWindow {
                max: 1,
                window_secs: 3_600,
            },
            expires_after_secs: 7_776_000,
            dark_window: None,
        },
    );
    insert_source(&mut registry, "standing_rule", "erased-rule", 1);

    registry.templates.insert(
        "erased-template".into(),
        PromptTemplate {
            id: "erased-template".into(),
            schema_version: 1,
            version: 1,
            lifecycle_state: Lifecycle::Active,
            system_preamble: "erased".into(),
            untrusted_data_preamble: None,
        },
    );
    insert_source(&mut registry, "template", "erased-template", 1);

    registry.personas.insert(
        "erased-persona".into(),
        PersonaElement {
            id: "erased-persona".into(),
            schema_version: 1,
            version: 1,
            lifecycle_state: Lifecycle::Active,
            guidance: "erased".into(),
        },
    );
    insert_source(&mut registry, "persona", "erased-persona", 1);

    let learned = vec![
        erased("route", "erased-route", 1),
        erased("agent", "erased-agent", 1),
        erased("workflow", "erased-workflow", 1),
        erased("pack", "erased-pack", 1),
        erased("policy", "erased-policy", 1),
        // wrong-version erase must not drop live base v2
        erased("model_swap", "base", 1),
        erased("model_swap", "matcher", 1),
        erased("standing_rule", "erased-rule", 1),
        erased("template", "erased-template", 1),
        erased("persona", "erased-persona", 1),
    ];

    exclude_erased(&mut registry, &learned);

    assert!(registry.routes.is_empty());
    assert!(!registry.agents.contains_key("erased-agent"));
    assert!(!registry.workflows.contains_key("erased-workflow"));
    assert!(!registry.packs.contains_key("erased-pack"));
    assert!(!registry.policies.contains_key("erased-policy"));
    assert!(!registry.standing_rules.contains_key("erased-rule"));
    assert!(!registry.templates.contains_key("erased-template"));
    assert!(!registry.personas.contains_key("erased-persona"));

    // Exact-version model_swap: matcher v1 erased; base remains because live
    // registry holds v2 while the erased row named v1.
    assert!(!registry.model_swaps.contains_key("matcher"));
    assert_eq!(registry.model_swaps["base"].version, 2);
    assert!(!registry
        .sources
        .contains_key(&("model_swap".into(), "matcher".into(), 1)));
    assert!(!registry
        .sources
        .contains_key(&("model_swap".into(), "base".into(), 1)));
    assert!(registry
        .sources
        .contains_key(&("model_swap".into(), "base".into(), 2)));

    for (kind, id) in [
        ("route", "erased-route"),
        ("agent", "erased-agent"),
        ("workflow", "erased-workflow"),
        ("pack", "erased-pack"),
        ("policy", "erased-policy"),
        ("standing_rule", "erased-rule"),
        ("template", "erased-template"),
        ("persona", "erased-persona"),
    ] {
        assert!(
            !registry.sources.contains_key(&(kind.into(), id.into(), 1)),
            "source for erased {kind}:{id} v1 must be removed"
        );
    }
}
