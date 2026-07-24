//! Deterministic resolution of an ambiguous route tie through
//! authority-equivalence classes (D-109/D-110; AD-147/AD-124).
//!
//! `resolve_route` reports a tie as `RouteResolution::Ambiguous { candidate_route_ids }`
//! using only its own deterministic conflict-resolution algorithm (D-008) —
//! it never decides authority. This module turns that tie into a final
//! decision *without* inventing new authority semantics: it composes each
//! tied candidate through the same kernel `compose_authority` path, groups
//! them by their [`AuthorityClassId`], and then lets
//! `AuthorityEquivalenceClasses::resolve` decide:
//!
//! - exactly one resulting class -> pick within the class deterministically
//!   (lowest `candidate id`; the class is authority-identical, so the pick
//!   cannot widen authority);
//! - more than one class -> escalate to the owner and never auto-pick.
//!
//! Candidate metadata or composition failures also escalate; dropping an
//! invalid competitor could otherwise conceal a cross-class outcome.

use jiff::Timestamp;
use openspine_authority::{AuthorityEquivalenceClasses, AuthorityInput, ClassResolution};
use openspine_schemas::action::ActionCatalog;
use openspine_schemas::agent::AgentManifest;
use openspine_schemas::event::EventEnvelope;
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::identity::{IdentityResolution, RelationshipKind};
use openspine_schemas::ids::ArtifactId;
use openspine_schemas::pack::CapabilityPack;
use openspine_schemas::route::Route;
use openspine_schemas::workflow::WorkflowManifest;
use ulid::Ulid;

use super::{empty_session_policy, AppState};

/// The audited authority artifacts a single route resolves to. The `route` is
/// held so the caller can later bind persona from it.
struct RouteAuthoritySources {
    route: Route,
    agent: AgentManifest,
    workflow: WorkflowManifest,
    pack: CapabilityPack,
}

/// The exact route and composed grant snapshot that passed equivalence
/// resolution. The driver must persist this grant rather than recomposing
/// against a potentially newer live registry.
pub(crate) struct SelectedRoute {
    pub route: Route,
    pub grant: TaskGrant,
}

/// How an ambiguous route tie was resolved.
pub enum TieResolution {
    /// Exactly one class resulted; the selected composition snapshot is
    /// carried into the grant path.
    Selected {
        selection: Box<SelectedRoute>,
        class_id: String,
    },
    /// No tied candidate's pack applies to this event — treat as a non-match.
    NotApplicable { route_ids: Vec<ArtifactId> },
    /// The tie spans more than one authority-equivalence class; surface to the
    /// owner and let them decide. Never auto-pick across classes (D-110).
    Escalate {
        route_ids: Vec<ArtifactId>,
        detail: String,
    },
}

/// Resolve a set of candidate route ids that `resolve_route` could not order,
/// using the sealed kernel equivalence-class path.
///
/// `candidate_route_ids` are already sorted and deduped by `resolve_route`.
/// Composition uses the same `identity`, `principal_id`, `purpose`, and `now`
/// the real single-route path would use, so a selected candidate composes the
/// identical grant the ordinary path would mint.
#[allow(clippy::too_many_arguments)]
pub fn resolve_tied_routes(
    state: &AppState,
    candidate_route_ids: &[ArtifactId],
    routes: &[Route],
    event: &EventEnvelope,
    identity: &IdentityResolution,
    relationship: Option<RelationshipKind>,
    action_catalog: &ActionCatalog,
    principal_id: Ulid,
    purpose: &str,
    now: Timestamp,
) -> anyhow::Result<TieResolution> {
    let registry = state.registry.read();
    let Some(global_policy) = registry.policies.get("global").cloned() else {
        return Ok(TieResolution::Escalate {
            route_ids: candidate_route_ids.to_vec(),
            detail: "global policy is missing while resolving tied routes".to_string(),
        });
    };

    // Assemble the authority sources for every applicable tied candidate.
    // A non-applicable pack is a non-match. Missing candidate metadata is
    // authority-relevant and must escalate rather than silently shrinking the
    // competing set.
    let mut applicable: Vec<RouteAuthoritySources> = Vec::new();
    for id in candidate_route_ids {
        let Some(route) = routes.iter().find(|r| &r.id == id).cloned() else {
            return Ok(TieResolution::Escalate {
                route_ids: candidate_route_ids.to_vec(),
                detail: format!("tied route {id} is missing from the resolved route set"),
            });
        };
        let Some(agent_id) = &route.agent else {
            return Ok(TieResolution::Escalate {
                route_ids: candidate_route_ids.to_vec(),
                detail: format!("tied route {id} names no agent"),
            });
        };
        let Some(workflow_id) = &route.workflow else {
            return Ok(TieResolution::Escalate {
                route_ids: candidate_route_ids.to_vec(),
                detail: format!("tied route {id} names no workflow"),
            });
        };
        let Some(pack_id) = &route.capability_pack else {
            return Ok(TieResolution::Escalate {
                route_ids: candidate_route_ids.to_vec(),
                detail: format!("tied route {id} names no capability pack"),
            });
        };
        let Some(agent) = registry.agents.get(agent_id).cloned() else {
            return Ok(TieResolution::Escalate {
                route_ids: candidate_route_ids.to_vec(),
                detail: format!("tied route {id} references missing agent {agent_id}"),
            });
        };
        let Some(workflow) = registry.workflows.get(workflow_id).cloned() else {
            return Ok(TieResolution::Escalate {
                route_ids: candidate_route_ids.to_vec(),
                detail: format!("tied route {id} references missing workflow {workflow_id}"),
            });
        };
        let Some(pack) = registry.packs.get(pack_id).cloned() else {
            return Ok(TieResolution::Escalate {
                route_ids: candidate_route_ids.to_vec(),
                detail: format!("tied route {id} references missing capability pack {pack_id}"),
            });
        };
        if !pack.applies_to.matches(event, relationship) {
            continue;
        }
        applicable.push(RouteAuthoritySources {
            route,
            agent,
            workflow,
            pack,
        });
    }
    drop(registry);

    if applicable.is_empty() {
        return Ok(TieResolution::NotApplicable {
            route_ids: candidate_route_ids.to_vec(),
        });
    }

    let session = empty_session_policy();
    let inputs: Vec<_> = applicable
        .iter()
        .map(|sources| {
            (
                sources.route.id.clone(),
                AuthorityInput {
                    event,
                    identity,
                    route: &sources.route,
                    global_policy: &global_policy,
                    agent: &sources.agent,
                    workflow: &sources.workflow,
                    pack: &sources.pack,
                    session: &session,
                    principal_id,
                    purpose,
                },
                sources.route.clone(),
            )
        })
        .collect();

    let borrowed = inputs
        .iter()
        .map(|(id, input, route)| (id.clone(), input, route.clone()));
    let classes = match AuthorityEquivalenceClasses::compose_all(action_catalog, borrowed, now) {
        Ok(classes) => classes,
        // A competing candidate failed to compose (denied / unknown action).
        // Do not conceal that authority-relevant outcome by picking the rest.
        Err(error) => {
            return Ok(TieResolution::Escalate {
                route_ids: candidate_route_ids.to_vec(),
                detail: format!("tied route composition failed: {error}"),
            });
        }
    };

    // `borrowed` were built from `classes`, so every present class is known;
    // resolve over the full set: one class -> deterministic pick, more ->
    // escalate.
    let known_class_ids: Vec<_> = classes.classes().map(|c| c.id().clone()).collect();
    match classes.resolve(&known_class_ids) {
        ClassResolution::NoMatch => Ok(TieResolution::NotApplicable {
            route_ids: candidate_route_ids.to_vec(),
        }),
        ClassResolution::Escalate { class_ids } => Ok(TieResolution::Escalate {
            route_ids: candidate_route_ids.to_vec(),
            detail: format!("tie spans {} authority classes", class_ids.len()),
        }),
        ClassResolution::Selected(resolved) => {
            // AD-147's frozen five-field class identity omits rated egress,
            // which the live gate enforces. Fail closed unless every
            // member has the same canonical egress set; never let an omitted
            // authority dimension turn a cross-egress tie into an auto-pick.
            let mut baseline_egress = None;
            for index in 0..resolved.len() {
                let Some(member) = resolved.select_within_class(|_| Some(index)) else {
                    return Ok(TieResolution::Escalate {
                        route_ids: candidate_route_ids.to_vec(),
                        detail: "selected authority class unexpectedly had no members".to_string(),
                    });
                };
                let mut egress = member.grant().allowed_egress_classes.clone();
                egress.sort();
                egress.dedup();
                if let Some(baseline) = &baseline_egress {
                    if baseline != &egress {
                        return Ok(TieResolution::Escalate {
                            route_ids: candidate_route_ids.to_vec(),
                            detail: "one authority class contains different effective egress sets"
                                .to_string(),
                        });
                    }
                } else {
                    baseline_egress = Some(egress);
                }
            }

            let member = resolved.select_within_class(|scope| {
                // Deterministic pick: the first member per `from_candidates`
                // ordering is the lexicographically smallest candidate id.
                (!scope.is_empty()).then_some(0)
            });
            match member {
                Some(member) => Ok(TieResolution::Selected {
                    selection: Box::new(SelectedRoute {
                        route: member.value().clone(),
                        grant: member.grant().clone(),
                    }),
                    class_id: format!("{:?}", resolved.id()),
                }),
                None => Ok(TieResolution::Escalate {
                    route_ids: candidate_route_ids.to_vec(),
                    detail: "selected authority class unexpectedly had no members".to_string(),
                }),
            }
        }
    }
}

/// Resolve an ambiguous route and perform the existing owner escalation side
/// effects for non-selected outcomes. `None` means the pipeline must return
/// without composing a grant.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn resolve_ambiguous_route(
    state: &AppState,
    candidate_route_ids: &[ArtifactId],
    routes: &[Route],
    event: &EventEnvelope,
    identity: &IdentityResolution,
    relationship: Option<RelationshipKind>,
    action_catalog: &ActionCatalog,
    principal_id: Ulid,
    purpose: &str,
    now: Timestamp,
    chat_id: i64,
) -> anyhow::Result<Option<SelectedRoute>> {
    let resolution = resolve_tied_routes(
        state,
        candidate_route_ids,
        routes,
        event,
        identity,
        relationship,
        action_catalog,
        principal_id,
        purpose,
        now,
    )?;
    match resolution {
        TieResolution::Selected {
            selection,
            class_id,
        } => {
            let detail = format!(
                "selected route {} within authority class {class_id}",
                selection.route.id
            );
            state.store.append_audit(
                "route.ambiguous.class_selected",
                None,
                None,
                Some(&detail),
                None,
                &[],
                &[],
            )?;
            Ok(Some(*selection))
        }
        TieResolution::NotApplicable { route_ids } => {
            let detail = format!(
                "tied candidate routes {route_ids:?} have no pack applicable to this event"
            );
            state.store.append_audit(
                "route.ambiguous.not_applicable",
                None,
                None,
                Some(&detail),
                None,
                &[],
                &[],
            )?;
            Ok(None)
        }
        TieResolution::Escalate { route_ids, detail } => {
            let summary = format!(
                "ambiguous route tie among {route_ids:?}: {detail}; owner decision required"
            );
            state.store.append_audit(
                "route.ambiguous.escalated",
                None,
                None,
                Some(&summary),
                None,
                &[],
                &[],
            )?;
            crate::failure_surfacing::notify_immediate_failure(
                state,
                chat_id,
                crate::failure_surfacing::FailureClass::Escalation,
                &summary,
            )
            .await?;
            Ok(None)
        }
    }
}
