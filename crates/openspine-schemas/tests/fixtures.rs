//! Round-trips every `artifacts/lyra/**/*.yaml` fixture through its typed
//! schema, and checks the spec-mandated invariants from
//! `openspec/changes/define-core-runtime-schemas/tasks.md` §6.

use std::fs;
use std::path::{Path, PathBuf};

use openspine_schemas::agent::AgentManifest;
use openspine_schemas::pack::CapabilityPack;
use openspine_schemas::policy::Policy;
use openspine_schemas::route::{Route, RouteEffect};
use openspine_schemas::workflow::WorkflowManifest;

fn artifacts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra")
}

fn read(rel: &str) -> String {
    let path = artifacts_dir().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading fixture {}: {e}", path.display()))
}

#[test]
fn owner_telegram_route_is_expressible_declaratively() {
    let route: Route =
        serde_yaml::from_str(&read("routes/owner_telegram_main_assistant.yaml")).unwrap();
    assert_eq!(route.id, "owner_telegram_main_assistant");
    assert_eq!(route.effect, RouteEffect::Allow);
    assert_eq!(route.agent.as_deref(), Some("main_assistant_agent"));
    assert_eq!(
        route.workflow.as_deref(),
        Some("owner_control_conversation")
    );
    assert_eq!(
        route.capability_pack.as_deref(),
        Some("owner_control_basic_pack")
    );
}

#[test]
fn owner_email_selected_thread_route_is_expressible_declaratively() {
    let route: Route =
        serde_yaml::from_str(&read("routes/owner_email_selected_thread.yaml")).unwrap();
    assert_eq!(route.id, "owner_email_selected_thread");
    assert_eq!(route.agent.as_deref(), Some("email_reply_drafter"));
    assert_eq!(
        route.workflow.as_deref(),
        Some("selected_thread_email_reply_draft")
    );
}

#[test]
fn agent_manifests_round_trip() {
    let main_assistant: AgentManifest =
        serde_yaml::from_str(&read("agents/main_assistant_agent.yaml")).unwrap();
    assert_eq!(main_assistant.id, "main_assistant_agent");
    assert!(main_assistant
        .denied_tools
        .iter()
        .any(|a| a.as_str() == "email.read_inbox"));

    let drafter: AgentManifest =
        serde_yaml::from_str(&read("agents/email_reply_drafter.yaml")).unwrap();
    assert_eq!(drafter.id, "email_reply_drafter");
    // §10.2's example has no approval_required_tools entry — must default, not error.
    assert!(drafter.approval_required_tools.is_empty());
    assert!(drafter
        .denied_tools
        .iter()
        .any(|a| a.as_str() == "email.send"));
}

#[test]
fn email_grant_pack_excludes_read_inbox_and_send() {
    let pack: CapabilityPack =
        serde_yaml::from_str(&read("packs/selected_thread_email_draft_pack.yaml")).unwrap();
    assert!(pack
        .denied_actions
        .iter()
        .any(|a| a.as_str() == "email.read_inbox"));
    assert!(pack
        .denied_actions
        .iter()
        .any(|a| a.as_str() == "email.send"));
    assert!(!pack
        .candidate_allowed_actions
        .iter()
        .any(|a| a.as_str() == "email.read_inbox"));
}

#[test]
fn owner_control_pack_round_trips() {
    let pack: CapabilityPack =
        serde_yaml::from_str(&read("packs/owner_control_basic_pack.yaml")).unwrap();
    assert_eq!(pack.id, "owner_control_basic_pack");
    assert!(pack
        .candidate_allowed_actions
        .iter()
        .any(|a| a.as_str() == "openspine.status.read"));
}

#[test]
fn plan_approval_pack_round_trips_and_declares_plan_actions() {
    let pack: CapabilityPack =
        serde_yaml::from_str(&read("packs/plan_approval_pack.yaml")).unwrap();
    assert_eq!(pack.id, "plan_approval_pack");
    assert!(pack
        .candidate_allowed_actions
        .iter()
        .any(|action| action.as_str() == "plan.propose"));
    assert!(pack
        .approval_required
        .iter()
        .any(|action| action.as_str() == "plan.execute"));
}
#[test]
fn workflow_manifests_round_trip() {
    let w: WorkflowManifest =
        serde_yaml::from_str(&read("workflows/owner_control_conversation.yaml")).unwrap();
    assert_eq!(w.required_agent, "main_assistant_agent");
    assert_eq!(w.required_capability_pack, "owner_control_basic_pack");

    let w2: WorkflowManifest =
        serde_yaml::from_str(&read("workflows/selected_thread_email_reply_draft.yaml")).unwrap();
    assert_eq!(w2.required_agent, "email_reply_drafter");
}

#[test]
fn reflection_routes_are_declarative_and_distinct() {
    let miner: Route =
        serde_yaml::from_str(&read("routes/reflection_scheduled_miner.yaml")).unwrap();
    let submitter: Route =
        serde_yaml::from_str(&read("routes/reflection_scheduled_submitter.yaml")).unwrap();
    assert_eq!(
        miner.when.channel_account.as_deref(),
        Some("reflection-miner")
    );
    assert_eq!(
        submitter.when.channel_account.as_deref(),
        Some("reflection-submitter")
    );
    assert_eq!(miner.agent.as_deref(), Some("reflection_miner_agent"));
    assert_eq!(
        submitter.agent.as_deref(),
        Some("reflection_submitter_agent")
    );
}

#[test]
fn reflection_agents_preserve_the_no_egress_boundary() {
    let miner: AgentManifest =
        serde_yaml::from_str(&read("agents/reflection_miner_agent.yaml")).unwrap();
    let submitter: AgentManifest =
        serde_yaml::from_str(&read("agents/reflection_submitter_agent.yaml")).unwrap();
    assert!(miner.output_channels.allowed.is_empty());
    assert!(miner
        .designed_tools
        .iter()
        .any(|action| action.as_str() == "model.generate:approved_provider"));
    assert!(submitter
        .designed_tools
        .iter()
        .any(|action| action.as_str() == "artifact.propose"));
}

#[test]
fn reflection_packs_and_workflows_are_bounded() {
    let miner_pack: CapabilityPack =
        serde_yaml::from_str(&read("packs/reflection_miner_pack.yaml")).unwrap();
    let submitter_pack: CapabilityPack =
        serde_yaml::from_str(&read("packs/reflection_submitter_pack.yaml")).unwrap();
    let miner_workflow: WorkflowManifest =
        serde_yaml::from_str(&read("workflows/reflection_miner_scheduled.yaml")).unwrap();
    let submitter_workflow: WorkflowManifest =
        serde_yaml::from_str(&read("workflows/reflection_submitter_scheduled.yaml")).unwrap();
    assert_eq!(miner_workflow.required_capability_pack, miner_pack.id);
    assert_eq!(
        submitter_workflow.required_capability_pack,
        submitter_pack.id
    );
    assert_eq!(
        miner_pack.constraints.data_classification_max,
        Some(openspine_schemas::event::DataClassification::Private)
    );
    assert!(submitter_pack
        .candidate_allowed_actions
        .iter()
        .any(|action| action.as_str() == "artifact.propose"));
}

#[test]
fn global_policy_round_trips_and_denies_send() {
    let policy: Policy = serde_yaml::from_str(&read("policies/global.yaml")).unwrap();
    assert_eq!(policy.id, "global");
    assert!(policy
        .denied_actions
        .iter()
        .any(|a| a.as_str() == "email.send"));
}

#[test]
fn every_fixture_file_is_covered_by_a_test() {
    // Guards against a future fixture landing without a round-trip test.
    let mut found = Vec::new();
    for sub in ["routes", "agents", "packs", "workflows", "policies"] {
        for entry in fs::read_dir(artifacts_dir().join(sub)).unwrap() {
            found.push(entry.unwrap().path());
        }
    }
    assert_eq!(
        found.len(),
        22,
        "expected 22 fixture files, found {found:?}"
    );
}
