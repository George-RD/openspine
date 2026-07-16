//! Authority composition (PRD §8, design.md's merge rule, spec.md).
//!
//! `compose_authority` is a pure function: no I/O, no storage lookups. The
//! caller has already run [`crate::resolve_route`] and resolved the agent,
//! workflow, capability pack, and policies it names — this function only
//! merges what it is handed.
//!
//! ## Resolving one ambiguity in design.md's merge rule
//!
//! Design.md step 3 says "intersect candidate allows with global policy and
//! user/session policy." Taken literally, an *empty* `candidate_allowed_actions`
//! on the global policy would intersect every action away — but this
//! product's global policy fixture (`artifacts/lyra/policies/global.yaml`)
//! deliberately carries only cross-cutting *denies*, with an empty allow
//! list, because enumerating every agent/workflow/pack's allowed actions in
//! one global artifact would fight D-013 ("dynamic behavior should be
//! easy"). This function therefore treats an empty policy allow-list as "no
//! additional narrowing" and a *non-empty* one as a real whitelist filter.
//! Recorded as part of this change's tasks.md, not a separate decision-log
//! entry (it clarifies design.md, it does not reverse an accepted decision).

use std::collections::HashSet;
use std::time::Duration;

use openspine_schemas::action::{ActionCatalog, ActionId};
use openspine_schemas::agent::AgentManifest;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::event::{DataClassification, EventEnvelope};
use openspine_schemas::grant::{GrantLimits, TaskGrant};
use openspine_schemas::identity::IdentityResolution;
use openspine_schemas::pack::CapabilityPack;
use openspine_schemas::policy::{Policy, SessionPolicy};
use openspine_schemas::route::{Route, RouteEffect};
use openspine_schemas::workflow::WorkflowManifest;
use rand::Rng;
use ulid::Ulid;

/// Every authority source design.md's merge rule composes over (PRD §8.1).
/// Borrowed, not owned: this is an in-process bundle, not a persisted
/// artifact — it has no `schema_version` of its own.
pub struct AuthorityInput<'a> {
    pub event: &'a EventEnvelope,
    pub identity: &'a IdentityResolution,
    pub route: &'a Route,
    pub global_policy: &'a Policy,
    pub agent: &'a AgentManifest,
    pub workflow: &'a WorkflowManifest,
    pub pack: &'a CapabilityPack,
    pub session: &'a SessionPolicy,
    /// The principal id the resulting task grant is issued to (AD-146).
    pub principal_id: Ulid,
    /// The specific task purpose (PRD §12's `purpose`, e.g.
    /// `draft_reply_for_selected_email_thread`) — distinct from
    /// `workflow.purpose`, which is a general description of what the
    /// workflow does, not a per-task slug.
    pub purpose: &'a str,
}

/// The result of one authority composition attempt.
#[derive(Debug, Clone, PartialEq)]
pub enum AuthorityOutcome {
    Granted(Box<TaskGrant>),
    Denied {
        reason: String,
    },
    /// Composition failed because a candidate action id is not in the
    /// canonical [`ActionCatalog`] (D-053): the id is outside the action
    /// universe, so no grant is minted. Carries the offending id and the
    /// source artifact / list that named it (agent `designed_tools` /
    /// `approval_required_tools` / `denied_tools`, workflow / pack / policy
    /// allow / approval / deny lists).
    UnknownActionId {
        id: ActionId,
        source: String,
    },
    /// Reserved for API symmetry with [`openspine_schemas::route::RouteResolution`].
    /// `compose_authority` receives an already-resolved single route, so it
    /// has no natural trigger for this today — ambiguity is resolved
    /// upstream by `resolve_route`.
    Ambiguous {
        fallback_route: String,
    },
}

fn classification_rank(c: DataClassification) -> u8 {
    match c {
        DataClassification::Public => 0,
        DataClassification::Internal => 1,
        DataClassification::Private => 2,
        DataClassification::Unknown => 3,
    }
}

/// Intersect `candidate` with `policy_allow`, unless `policy_allow` is
/// empty (see the module-level note on design.md step 3).
fn narrow(candidate: HashSet<ActionId>, policy_allow: &[ActionId]) -> HashSet<ActionId> {
    if policy_allow.is_empty() {
        return candidate;
    }
    let allowed: HashSet<&ActionId> = policy_allow.iter().collect();
    candidate
        .into_iter()
        .filter(|a| allowed.contains(a))
        .collect()
}

fn mint_task_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Validate that every candidate action id in the composition sources
/// belongs to `catalog` (D-053). Returns the first unknown id and the
/// source list that named it, so composition fails fast with a structured
/// error instead of minting a grant that smuggles an unrecognized id into
/// the authority universe.
fn unknown_candidate<'a>(
    input: &'a AuthorityInput<'a>,
    catalog: &ActionCatalog,
) -> Option<(ActionId, String)> {
    let sources: &[(&[ActionId], &str)] = &[
        (&input.agent.designed_tools, "agent.designed_tools"),
        (
            &input.agent.approval_required_tools,
            "agent.approval_required_tools",
        ),
        (&input.agent.denied_tools, "agent.denied_tools"),
        (
            &input.workflow.candidate_allowed_actions,
            "workflow.candidate_allowed_actions",
        ),
        (
            &input.workflow.approval_required,
            "workflow.approval_required",
        ),
        (&input.workflow.denied_actions, "workflow.denied_actions"),
        (
            &input.pack.candidate_allowed_actions,
            "pack.candidate_allowed_actions",
        ),
        (&input.pack.approval_required, "pack.approval_required"),
        (&input.pack.denied_actions, "pack.denied_actions"),
        (
            &input.global_policy.candidate_allowed_actions,
            "global_policy.candidate_allowed_actions",
        ),
        (
            &input.global_policy.approval_required,
            "global_policy.approval_required",
        ),
        (
            &input.global_policy.denied_actions,
            "global_policy.denied_actions",
        ),
        (
            &input.session.candidate_allowed_actions,
            "session.candidate_allowed_actions",
        ),
        (
            &input.session.approval_required,
            "session.approval_required",
        ),
        (&input.session.denied_actions, "session.denied_actions"),
    ];
    for (ids, label) in sources {
        for cand in *ids {
            if !catalog.contains(cand) {
                return Some((cand.clone(), (*label).to_string()));
            }
        }
    }
    None
}

/// Compose final task authority from every input source (PRD §8.2, design.md).
///
/// Precedence: explicit deny > approval-required > allow > unspecified
/// deny-by-default (PRD §8.3). An action absent from every source's
/// candidate-allow list is simply never granted — deny-by-default needs no
/// explicit entry.
pub fn compose_authority(
    input: &AuthorityInput,
    catalog: &ActionCatalog,
    now: jiff::Timestamp,
) -> AuthorityOutcome {
    // Quarantined/non-active artifacts cannot participate in task grants
    // (PRD §13.2) — this is also how "authority widening requires approval"
    // (PRD §8.4) is enforced: a proposed/review-required artifact simply
    // never reaches Active, so it can never be composed into a grant.
    for (kind, lifecycle) in [
        ("route", input.route.lifecycle_state),
        ("agent", input.agent.lifecycle_state),
        ("workflow", input.workflow.lifecycle_state),
        ("capability_pack", input.pack.lifecycle_state),
        ("global_policy", input.global_policy.lifecycle_state),
    ] {
        if lifecycle != Lifecycle::Active {
            return AuthorityOutcome::Denied {
                reason: format!("{kind} is not active (lifecycle_state={lifecycle:?})"),
            };
        }
    }

    if input.route.effect == RouteEffect::Deny {
        return AuthorityOutcome::Denied {
            reason: format!("route {} is a deny route", input.route.id),
        };
    }

    // Identity is not authority (D-006): a route requiring a verified
    // source must see genuine verification on both the event and the
    // identity resolution, never an identity match alone.
    if input.route.when.verified_source == Some(true)
        && !(input.event.verified_source && input.identity.source_verified)
    {
        return AuthorityOutcome::Denied {
            reason: "route requires a verified source, but the event/identity is not verified"
                .to_string(),
        };
    }

    // D-053: fail-fast on any candidate id outside the canonical catalog —
    // no grant may be minted for an action the kernel does not recognize.
    if let Some((unknown_id, source)) = unknown_candidate(input, catalog) {
        return AuthorityOutcome::UnknownActionId {
            id: unknown_id,
            source,
        };
    }
    // Steps 1-2: deny-by-default, then gather candidate allows.
    let mut allow: HashSet<ActionId> = HashSet::new();
    allow.extend(input.agent.designed_tools.iter().cloned());
    allow.extend(input.workflow.candidate_allowed_actions.iter().cloned());
    allow.extend(input.pack.candidate_allowed_actions.iter().cloned());

    // Step 3: intersect with global/session policy.
    allow = narrow(allow, &input.global_policy.candidate_allowed_actions);
    allow = narrow(allow, &input.session.candidate_allowed_actions);

    // Step 4: data-classification constraint (lane/connector/account-role/
    // channel constraints are already enforced by resolve_route's `when`
    // match before compose_authority ever runs on this route).
    if let Some(max) = input.pack.constraints.data_classification_max {
        if classification_rank(input.event.data_classification) > classification_rank(max) {
            return AuthorityOutcome::Denied {
                reason: format!(
                    "event data classification {:?} exceeds pack maximum {:?}",
                    input.event.data_classification, max
                ),
            };
        }
    }

    // Steps 5-6: gather explicit denies and approval-required actions.
    let mut deny: HashSet<ActionId> = HashSet::new();
    deny.extend(input.agent.denied_tools.iter().cloned());
    deny.extend(input.workflow.denied_actions.iter().cloned());
    deny.extend(input.pack.denied_actions.iter().cloned());
    deny.extend(input.global_policy.denied_actions.iter().cloned());
    deny.extend(input.session.denied_actions.iter().cloned());

    let mut approval_required: HashSet<ActionId> = HashSet::new();
    approval_required.extend(input.agent.approval_required_tools.iter().cloned());
    approval_required.extend(input.workflow.approval_required.iter().cloned());
    approval_required.extend(input.pack.approval_required.iter().cloned());
    approval_required.extend(input.global_policy.approval_required.iter().cloned());
    approval_required.extend(input.session.approval_required.iter().cloned());

    // Steps 7-8: precedence. Deny beats everything; approval-required beats plain allow.
    approval_required.retain(|a| !deny.contains(a));
    allow.retain(|a| !deny.contains(a) && !approval_required.contains(a));

    let max_runtime_seconds = match input.pack.constraints.max_runtime_seconds {
        Some(pack_limit) => pack_limit.min(input.agent.limits.max_runtime_seconds),
        None => input.agent.limits.max_runtime_seconds,
    };

    let issued_at = now;
    let expires_at = issued_at + Duration::from_secs(max_runtime_seconds);

    let mut allowed_actions: Vec<ActionId> = allow.into_iter().collect();
    allowed_actions.sort();
    let mut approval_required_actions: Vec<ActionId> = approval_required.into_iter().collect();
    approval_required_actions.sort();
    let mut denied_actions: Vec<ActionId> = deny.into_iter().collect();
    denied_actions.sort();

    let grant = TaskGrant {
        id: Ulid::new(),
        schema_version: 1,
        lifecycle_state: Lifecycle::Active,
        user: input.principal_id.to_string(),
        purpose: input.purpose.to_string(),
        issued_by: "kernel".to_string(),
        issued_at,
        expires_at,
        event_id: input.event.id,
        route_id: input.route.id.clone(),
        agent_id: input.agent.id.clone(),
        workflow_id: input.workflow.id.clone(),
        capability_pack_id: input.pack.id.clone(),
        authority_sources: vec![
            format!("global_policy:v{}", input.global_policy.version),
            format!("route:{}:v{}", input.route.id, input.route.version),
            format!("agent:{}:v{}", input.agent.id, input.agent.version),
            format!("workflow:{}:v{}", input.workflow.id, input.workflow.version),
            format!("capability_pack:{}:v{}", input.pack.id, input.pack.version),
        ],
        selection_tokens: vec![],
        allowed_actions,
        approval_required_actions,
        denied_actions,
        allowed_egress_classes: input.pack.allowed_egress_classes.clone(),
        output_channels: input.agent.output_channels.allowed.clone(),
        limits: GrantLimits {
            max_model_calls: input.agent.model_policy.max_model_calls_per_task,
            max_artifacts: input.agent.limits.max_artifacts,
            max_runtime_seconds,
        },
        task_token: mint_task_token(),
        root_grant_id: Ulid::nil(),
        parent_grant_id: None,
        mode: openspine_schemas::grant::GrantMode::Live,
        chain: vec![],
        caveat_mac: String::new(),
        thread_id: None,
    };

    AuthorityOutcome::Granted(Box::new(grant))
}
