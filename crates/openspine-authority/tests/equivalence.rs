//! Integration tests for `AuthorityEquivalenceClasses` (AD-147, AD-124).
//!
//! These exercise the sealed kernel path: candidates are composed through the
//! same `compose_authority` the kernel uses to mint a live grant, then
//! grouped by their deterministic class identity. The semantic matcher
//! receives only a class-scoped view, so a cross-class pick is
//! structurally impossible.
#[allow(dead_code)]
mod common;

use common::*;
use jiff::Timestamp;
use openspine_authority::{
    AuthorityEquivalenceClasses, AuthorityInput, ClassResolution, EquivalenceError,
};
use openspine_schemas::action::{ActionCatalog, ActionId};
use openspine_schemas::agent::AgentManifest;
use openspine_schemas::event::EventEnvelope;
use openspine_schemas::identity::IdentityResolution;
use openspine_schemas::pack::CapabilityPack;
use openspine_schemas::policy::{Policy, SessionPolicy};
use openspine_schemas::route::Route;
use openspine_schemas::workflow::WorkflowManifest;

/// Shared fixtures. Pack/workflow allowed lists are emptied so the agent's
/// `designed_tools` is the sole source of composed `allowed_actions`, making
/// distinct designed-tool subsets produce distinct authority classes.
struct Harness {
    event: EventEnvelope,
    identity: IdentityResolution,
    route: Route,
    workflow: WorkflowManifest,
    pack: CapabilityPack,
    policy: Policy,
    session: SessionPolicy,
    catalog: ActionCatalog,
    agents: Vec<AgentManifest>,
}

impl Harness {
    fn new() -> Self {
        let mut workflow = owner_control_conversation_workflow();
        workflow.candidate_allowed_actions = vec![];
        workflow.approval_required = vec![];
        workflow.denied_actions = vec![];
        let mut pack = owner_control_basic_pack();
        pack.candidate_allowed_actions = vec![];
        pack.approval_required = vec![];
        pack.denied_actions = vec![];
        pack.constraints.max_runtime_seconds = None;
        Harness {
            event: owner_event(),
            identity: owner_identity(),
            route: owner_route(),
            workflow,
            pack,
            policy: global_policy(),
            session: empty_session_policy(),
            catalog: test_catalog(),
            agents: Vec::new(),
        }
    }

    /// Register an agent whose `designed_tools` are exactly `allowed`, and
    /// return its index for later `input`.
    fn agent_with(&mut self, allowed: &[&str]) -> usize {
        let mut agent = main_assistant_agent();
        agent.designed_tools = allowed.iter().map(|s| ActionId::new(*s)).collect();
        self.agents.push(agent);
        self.agents.len() - 1
    }

    /// Build one-input variants that change exactly one composed authority
    /// dimension, while preserving the same valid action catalog.
    fn variant(&mut self, allowed: &[&str], dimension: usize) -> usize {
        let mut agent = main_assistant_agent();
        agent.designed_tools = allowed.iter().map(|s| ActionId::new(*s)).collect();
        match dimension {
            1 => {
                agent.approval_required_tools = vec![ActionId::new("artifact.propose")];
            }
            2 => agent
                .denied_tools
                .push(ActionId::new("artifact.nominate_upstream")),
            3 => {
                agent.output_channels.allowed = vec![
                    "telegram.owner.reply".to_string(),
                    "email.draft".to_string(),
                ];
            }
            4 => agent.limits.max_artifacts += 1,
            5 => agent.limits.max_runtime_seconds += 1,
            _ => {}
        }
        self.agents.push(agent);
        self.agents.len() - 1
    }

    /// Compose an `AuthorityInput` borrowing harness fixtures + agent `idx`.
    fn input(&self, idx: usize) -> AuthorityInput<'_> {
        owner_control_input(
            &self.event,
            &self.identity,
            &self.route,
            &self.agents[idx],
            &self.workflow,
            &self.pack,
            &self.policy,
            &self.session,
        )
    }
}

/// The stable, catalog-known action ids that survive composition (they are not
/// present in any fixture denial list), so varying subsets of them yields
/// distinct composed authority classes.
fn safe_ids() -> Vec<ActionId> {
    vec![
        ActionId::new("openspine.status.read"),
        ActionId::new("telegram.reply:owner_channel"),
        ActionId::new("workflow.invoke:approved"),
        ActionId::new("artifact.propose"),
        ActionId::new("artifact.nominate_upstream"),
    ]
}

#[test]
fn property_all_authority_dimensions_define_classes_and_identical_grants() {
    let mut h = Harness::new();
    let allowed = ["openspine.status.read", "artifact.propose"];
    let mut agents = Vec::new();
    for dimension in 0..=7 {
        let index = if dimension == 0 {
            h.agent_with(&allowed)
        } else if dimension == 7 {
            h.variant(&["openspine.status.read"], 0)
        } else {
            h.variant(&allowed, dimension)
        };
        agents.push(index);
    }
    h.agents[agents[6]].output_channels.allowed.reverse();
    let mut inputs = Vec::new();
    let mut labels = Vec::new();
    for (dimension, agent) in agents.iter().enumerate() {
        inputs.push(h.input(*agent));
        inputs.push(h.input(*agent));
        labels.push(format!("d{dimension}-a"));
        labels.push(format!("d{dimension}-b"));
    }
    let tuples: Vec<(String, &AuthorityInput<'_>, ())> = labels
        .into_iter()
        .zip(inputs.iter())
        .map(|(id, input)| (id, input, ()))
        .collect();
    let classes = AuthorityEquivalenceClasses::compose_all(&h.catalog, tuples, Timestamp::now())
        .expect("all dimension variants compose");
    let partitions: Vec<Vec<String>> = classes
        .classes()
        .map(|class| class.candidate_ids().map(str::to_owned).collect())
        .collect();
    assert_eq!(classes.class_count(), 7, "partitions={partitions:?}");
    let merged: Vec<Vec<String>> = classes
        .classes()
        .filter(|class| class.len() == 4)
        .map(|class| class.candidate_ids().map(str::to_owned).collect())
        .collect();
    assert_eq!(
        merged,
        vec![vec![
            "d0-a".to_string(),
            "d0-b".to_string(),
            "d6-a".to_string(),
            "d6-b".to_string(),
        ]]
    );
    for class in classes.classes() {
        assert!(class.len() == 2 || class.len() == 4);
        let ClassResolution::Selected(selected) = classes.resolve(&[class.id().clone()]) else {
            panic!("each class resolves uniquely");
        };
        let baseline = selected
            .select_within_class(|_| Some(0))
            .expect("member")
            .grant();
        for index in 0..class.len() {
            let member = selected
                .select_within_class(|_| Some(index))
                .expect("member");
            let grant = member.grant();
            assert_eq!(grant.allowed_actions, baseline.allowed_actions);
            assert_eq!(
                grant.approval_required_actions,
                baseline.approval_required_actions
            );
            assert_eq!(grant.denied_actions, baseline.denied_actions);
            assert_eq!(grant.output_channels, baseline.output_channels);
            assert_eq!(grant.limits, baseline.limits);
        }
    }
}

#[test]
fn property_within_class_pick_holds_identical_authority_projection() {
    let safe = safe_ids();
    let mut h = Harness::new();

    // Build one class per safe id, where class `i` contains exactly the
    // first `i+1` safe ids; class `i` gets `i+1` members.
    let mut agent_indexes: Vec<(usize, usize)> = Vec::new();
    for (i, _) in safe.iter().enumerate() {
        let idx = h.agent_with(&safe[..=i].iter().map(|a| a.as_str()).collect::<Vec<_>>());
        agent_indexes.push((idx, i + 1));
    }

    let mut inputs: Vec<AuthorityInput<'_>> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    for (agent_idx, members) in agent_indexes {
        for m in 0..members {
            labels.push(format!("c{agent_idx}-{m}"));
            inputs.push(h.input(agent_idx));
        }
    }

    let tuples: Vec<(String, &AuthorityInput<'_>, ())> = labels
        .into_iter()
        .zip(inputs.iter())
        .map(|(id, input)| (id, input, ()))
        .collect();

    let classes = AuthorityEquivalenceClasses::compose_all(&h.catalog, tuples, Timestamp::now())
        .expect("all candidates compose");

    // One class per safe id.
    assert_eq!(classes.class_count(), safe.len());

    // Property: every within-class pick returns a member of exactly that
    // class; the member's class identity equals the class key.
    for class in classes.classes() {
        let key = class.id().clone();
        let resolution = classes.resolve(std::slice::from_ref(&key));
        let ClassResolution::Selected(selected) = resolution else {
            panic!("one known class must resolve");
        };
        let baseline = selected
            .select_within_class(|_scope| Some(0))
            .expect("class has at least one member")
            .grant();
        for index in 0..class.len() {
            let member = selected
                .select_within_class(|scope| {
                    assert_eq!(scope.class_id(), &key);
                    Some(index)
                })
                .expect("index is within this class scope");
            assert_eq!(member.class_id(), &key);
            let g = member.grant();
            assert_eq!(g.allowed_actions, baseline.allowed_actions);
            assert_eq!(
                g.approval_required_actions,
                baseline.approval_required_actions
            );
            assert_eq!(g.denied_actions, baseline.denied_actions);
            assert_eq!(g.output_channels, baseline.output_channels);
            assert_eq!(g.limits, baseline.limits);
        }
    }

    // Cross-class resolution over every class is ambiguous (escalates), so
    // no single pick can ever cross a class boundary.
    let all_ids: Vec<_> = classes.classes().map(|c| c.id().clone()).collect();
    assert!(matches!(
        classes.resolve(&all_ids),
        ClassResolution::Escalate { .. }
    ));

    // A single class resolves deterministically to itself.
    assert!(matches!(
        classes.resolve(&[all_ids[0].clone()]),
        ClassResolution::Selected(_)
    ));
}

#[test]
fn two_identical_inputs_form_one_class() {
    let mut h = Harness::new();
    let a = h.agent_with(&["openspine.status.read", "telegram.reply:owner_channel"]);
    let inputs = [h.input(a), h.input(a)];
    let tuples: Vec<(String, &AuthorityInput<'_>, ())> = inputs
        .iter()
        .enumerate()
        .map(|(i, input)| (format!("c{i}"), input, ()))
        .collect();

    let classes = AuthorityEquivalenceClasses::compose_all(&h.catalog, tuples, Timestamp::now())
        .expect("all candidates compose");

    assert_eq!(classes.class_count(), 1);
    let only = classes.classes().next().expect("one class");
    assert_eq!(only.len(), 2);
    let ClassResolution::Selected(selected) = classes.resolve(&[only.id().clone()]) else {
        panic!("one known class must resolve");
    };
    let member = selected
        .select_within_class(|scope| {
            assert_eq!(scope.len(), 2);
            Some(0)
        })
        .expect("member 0 exists");
    assert_eq!(member.class_id(), only.id());
}

#[test]
fn distinct_inputs_form_separate_classes_and_escalate() {
    let mut h = Harness::new();
    let a0 = h.agent_with(&["openspine.status.read"]);
    let a1 = h.agent_with(&["telegram.reply:owner_channel"]);
    let inputs = [h.input(a0), h.input(a1)];
    let tuples: Vec<(String, &AuthorityInput<'_>, ())> = inputs
        .iter()
        .enumerate()
        .map(|(i, input)| (format!("c{i}"), input, ()))
        .collect();

    let classes = AuthorityEquivalenceClasses::compose_all(&h.catalog, tuples, Timestamp::now())
        .expect("all candidates compose");

    assert_eq!(classes.class_count(), 2);

    let ids: Vec<_> = classes.classes().map(|c| c.id().clone()).collect();
    assert!(matches!(
        classes.resolve(&ids),
        ClassResolution::Escalate { class_ids } if class_ids.len() == 2
    ));
}

#[test]
fn declared_list_order_does_not_change_class() {
    let mut h = Harness::new();
    let a0 = h.agent_with(&[
        "openspine.status.read",
        "telegram.reply:owner_channel",
        "artifact.propose",
    ]);
    let a1 = h.agent_with(&[
        "artifact.propose",
        "openspine.status.read",
        "telegram.reply:owner_channel",
    ]);
    let inputs = [h.input(a0), h.input(a1)];
    let tuples: Vec<(String, &AuthorityInput<'_>, ())> = inputs
        .iter()
        .enumerate()
        .map(|(i, input)| (format!("c{i}"), input, ()))
        .collect();

    let classes = AuthorityEquivalenceClasses::compose_all(&h.catalog, tuples, Timestamp::now())
        .expect("all candidates compose");

    // Same set, different declaration order -> one authority class.
    assert_eq!(classes.class_count(), 1);
}

#[test]
fn compose_denial_is_a_class_error() {
    let mut h = Harness::new();
    // An unknown action id fails composition fast (D-053) and surfaces as an
    // `EquivalenceError`, never as a silently-widened class.
    let bad = h.agent_with(&["openspine.status.read", "action.unknown_to_catalog"]);
    let inputs = [h.input(bad)];
    let tuples: Vec<(String, &AuthorityInput<'_>, ())> = inputs
        .iter()
        .enumerate()
        .map(|(i, input)| (format!("c{i}"), input, ()))
        .collect();

    let result = AuthorityEquivalenceClasses::compose_all(&h.catalog, tuples, Timestamp::now());
    assert!(matches!(
        result,
        Err(EquivalenceError::CompositionDenied(id))
            if id == "c0"
    ));
}

#[test]
fn class_identity_is_stable_across_composition_timestamps() {
    let mut h = Harness::new();
    let agent = h.agent_with(&["openspine.status.read", "artifact.propose"]);
    let input = h.input(agent);
    let tuples = [("same".to_string(), &input, ())];
    let first = AuthorityEquivalenceClasses::compose_all(
        &h.catalog,
        tuples
            .iter()
            .map(|(id, input, marker)| (id.clone(), *input, *marker)),
        Timestamp::from_second(1).unwrap(),
    )
    .expect("first composition");
    let second = AuthorityEquivalenceClasses::compose_all(
        &h.catalog,
        tuples
            .iter()
            .map(|(id, input, marker)| (id.clone(), *input, *marker)),
        Timestamp::from_second(2).unwrap(),
    )
    .expect("second composition");
    let first_id = first.classes().next().expect("first class").id().clone();
    let second_id = second.classes().next().expect("second class").id().clone();
    assert_eq!(first_id, second_id);
}
