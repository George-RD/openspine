//! Declarative route artifacts and route resolution (PRD §6).
//!
//! Routes are declarative artifacts — they map event/identity/context to a
//! candidate agent, workflow, and capability pack. A route never directly
//! grants final runtime authority (spec.md); only a task grant does.

use serde::{Deserialize, Serialize};

use crate::event::{AccountRole, Connector, EventType, Lane, Source};
use crate::identity::RelationshipKind;
use crate::ids::ArtifactId;

/// The `when.actor` sub-clause of a route.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RouteActorWhen {
    pub relationship: Option<RelationshipKind>,
    pub channel_trust: Option<crate::event::ChannelTrust>,
    /// Minimum identity-resolution confidence required, in `[0.0, 1.0]`.
    pub identity_confidence_min: Option<f64>,
}

/// The `when` match clause of a route (PRD §6.1, specificity fields per §6.3).
/// Every present field must match exactly — no semantic/LLM-based scoring
/// is allowed for authority decisions.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RouteWhen {
    pub source: Option<Source>,
    pub event_type: Option<EventType>,
    pub verified_source: Option<bool>,
    pub lane: Option<Lane>,
    pub connector: Option<Connector>,
    pub account_role: Option<AccountRole>,
    /// AD-136: the connector/number a conversation arrives on. A route
    /// matches only when the event's `channel_account` equals this value,
    /// so a bound number deterministically selects its route/persona.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_account: Option<String>,
    pub actor: Option<RouteActorWhen>,
}

impl RouteWhen {
    /// Specificity = count of explicit `when` fields set (PRD §6.3). Used
    /// only to break priority ties between matching routes — never as a
    /// substitute for an explicit `priority`.
    pub fn specificity(&self) -> u32 {
        let mut n = 0;
        if self.source.is_some() {
            n += 1;
        }
        if self.event_type.is_some() {
            n += 1;
        }
        if self.verified_source.is_some() {
            n += 1;
        }
        if self.lane.is_some() {
            n += 1;
        }
        if self.connector.is_some() {
            n += 1;
        }
        if self.account_role.is_some() {
            n += 1;
        }
        if self.channel_account.is_some() {
            n += 1;
        }
        if let Some(actor) = &self.actor {
            if actor.relationship.is_some() {
                n += 1;
            }
            if actor.channel_trust.is_some() {
                n += 1;
            }
            if actor.identity_confidence_min.is_some() {
                n += 1;
            }
        }
        n
    }
}

/// A route's effect once matched (PRD §6.2: "exact deny route wins over
/// allow route"). Defaults to `Allow` — the PRD's example routes are all
/// allow-routes and don't spell this field out explicitly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteEffect {
    #[default]
    Allow,
    Deny,
}

/// A declarative route artifact (PRD §6.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub id: ArtifactId,
    pub schema_version: u32,
    #[serde(default = "crate::artifact::default_version")]
    pub version: u32,
    pub lifecycle_state: crate::artifact::Lifecycle,
    pub priority: Option<u32>,
    #[serde(default)]
    pub effect: RouteEffect,
    #[serde(default)]
    pub when: RouteWhen,
    pub agent: Option<ArtifactId>,
    pub workflow: Option<ArtifactId>,
    pub capability_pack: Option<ArtifactId>,
    /// AD-136: the persona artifact this route selects when it wins.
    /// `None` means the route binds no persona. The binding is kernel
    /// machinery — this is only the *reference* the kernel carries
    /// from the matched route; an unbound or invalid persona yields
    /// no fronting persona (never the agent's choice).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona: Option<ArtifactId>,
}

/// The result of resolving one or more candidate routes against an event
/// (PRD §6.4). `Ambiguous` never grants widened authority — it always falls
/// back to `low_authority_triage` if a caller chooses to use it. Since
/// D-109/D-110, a caller may instead resolve the tie deterministically by
/// composing `candidate_route_ids` (the tied winners, sorted and deduped)
/// through `AuthorityEquivalenceClasses`: a single resulting class picks
/// within itself, more than one class escalates to the owner. Adoption only
/// — this field does not change `resolve_route`'s own deterministic
/// conflict-resolution algorithm (D-008).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RouteResolution {
    Success {
        route_id: ArtifactId,
    },
    Denied {
        reason: String,
    },
    Ambiguous {
        fallback_route: ArtifactId,
        reason: String,
        /// The tied route ids `resolve_route` could not order
        /// deterministically, sorted and deduplicated.
        candidate_route_ids: Vec<ArtifactId>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact::Lifecycle;

    fn owner_route() -> Route {
        Route {
            id: "owner_telegram_main_assistant".to_string(),
            schema_version: 1,
            version: 1,
            lifecycle_state: Lifecycle::Active,
            priority: Some(100),
            effect: RouteEffect::Allow,
            when: RouteWhen {
                source: Some(Source::Telegram),
                event_type: Some(EventType::TelegramOwnerMessage),
                verified_source: Some(true),
                lane: Some(Lane::OwnerControl),
                actor: Some(RouteActorWhen {
                    relationship: Some(RelationshipKind::Owner),
                    channel_trust: Some(crate::event::ChannelTrust::VerifiedOwnerChannel),
                    identity_confidence_min: Some(0.95),
                }),
                ..Default::default()
            },
            agent: Some("main_assistant_agent".to_string()),
            workflow: Some("owner_control_conversation".to_string()),
            capability_pack: Some("owner_control_basic_pack".to_string()),
            persona: None,
        }
    }

    #[test]
    fn round_trips_through_serde() {
        let route = owner_route();
        let json = serde_json::to_string(&route).unwrap();
        let back: Route = serde_json::from_str(&json).unwrap();
        assert_eq!(route, back);
    }

    #[test]
    fn effect_defaults_to_allow_when_omitted() {
        let mut json = serde_json::to_value(owner_route()).unwrap();
        json.as_object_mut().unwrap().remove("effect");
        let route: Route = serde_json::from_value(json).unwrap();
        assert_eq!(route.effect, RouteEffect::Allow);
    }

    #[test]
    fn specificity_counts_explicit_when_fields() {
        // owner_telegram_main_assistant sets: source, event_type,
        // verified_source, lane, actor.relationship, actor.channel_trust,
        // actor.identity_confidence_min = 7 explicit fields.
        assert_eq!(owner_route().when.specificity(), 7);
        assert_eq!(RouteWhen::default().specificity(), 0);
    }

    #[test]
    fn ambiguous_resolution_names_the_fallback_route_verbatim() {
        let resolution = RouteResolution::Ambiguous {
            fallback_route: "low_authority_triage".to_string(),
            reason: "multiple_matching_routes_no_deterministic_winner".to_string(),
            candidate_route_ids: vec!["a".to_string(), "b".to_string()],
        };
        let value = serde_json::to_value(&resolution).unwrap();
        assert_eq!(value["status"], "ambiguous");
        assert_eq!(value["fallback_route"], "low_authority_triage");
        assert_eq!(value["candidate_route_ids"], serde_json::json!(["a", "b"]));
    }

    #[test]
    fn route_can_be_a_deny_route() {
        let mut route = owner_route();
        route.effect = RouteEffect::Deny;
        let value = serde_json::to_value(&route).unwrap();
        assert_eq!(value["effect"], "deny");
    }
}
