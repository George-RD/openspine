//! Deterministic persona binding (AD-136), kernel machinery.
//!
//! WHICH persona fronts a conversation is decided here — never by the agent —
//! from the resolved route, the resolved identity, and the relationship kind,
//! exactly as route resolution already does. Because this runs only *after* a
//! deterministic route match, a counterparty reaching an owner-bound number
//! cannot select the owner persona: the binding derives from the matched
//! route, not from any agent-supplied input.

use std::collections::HashMap;

use openspine_schemas::identity::RelationshipKind;
use openspine_schemas::persona::PersonaElement;
use openspine_schemas::route::Route;

/// Resolve the persona artifact id a route fronts, or `None` when no
/// persona is bound (unbound/invalid → no fronting persona, never the
/// agent's choice — AD-136).
///
/// Returns `None` when:
/// - the winning `route.persona` is `None`, or
/// - the named persona is absent from the loaded `personas` registry, or
/// - the named persona is not `Active` (a quarantined/dormant persona
///   cannot front a conversation).
pub fn resolve_persona(
    event_channel_account: &str,
    relationship: Option<RelationshipKind>,
    route: &Route,
    personas: &HashMap<String, PersonaElement>,
) -> Option<String> {
    // Re-check the route's context at the binding boundary. Route selection
    // already performed this match, but repeating it here makes persona
    // binding structurally context-aware rather than a post-hoc id filter.
    if route
        .when
        .channel_account
        .as_deref()
        .is_some_and(|bound| bound != event_channel_account)
    {
        return None;
    }
    if route
        .when
        .actor
        .as_ref()
        .and_then(|actor| actor.relationship)
        .is_some_and(|bound| Some(bound) != relationship)
    {
        return None;
    }
    let Some(persona_id) = &route.persona else {
        return None;
    };
    let persona = personas.get(persona_id)?;
    if persona.lifecycle_state != openspine_schemas::artifact::Lifecycle::Active {
        return None;
    }
    // The relationship passed in is the *resolved* relationship from
    // identity resolution, never the agent's claim. Structural binding is
    // therefore impossible to subvert from the conversation side.
    Some(persona_id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspine_schemas::artifact::Lifecycle;
    use openspine_schemas::route::{RouteActorWhen, RouteEffect, RouteWhen};

    fn route(persona: Option<&str>) -> Route {
        Route {
            id: "owner-bound-number".to_string(),
            schema_version: 1,
            version: 1,
            lifecycle_state: Lifecycle::Active,
            priority: Some(10),
            effect: RouteEffect::Allow,
            when: RouteWhen::default(),
            agent: None,
            workflow: None,
            capability_pack: None,
            persona: persona.map(str::to_string),
        }
    }

    fn persona(id: &str) -> PersonaElement {
        PersonaElement {
            id: id.to_string(),
            schema_version: 1,
            version: 1,
            lifecycle_state: Lifecycle::Active,
            guidance: "owner-facing".to_string(),
        }
    }

    #[test]
    fn owner_bound_number_selects_route_persona() {
        let mut personas = HashMap::new();
        personas.insert("owner-facing".to_string(), persona("owner-facing"));
        let selected = resolve_persona(
            "owner-number",
            Some(RelationshipKind::Owner),
            &route(Some("owner-facing")),
            &personas,
        );
        assert_eq!(selected.as_deref(), Some("owner-facing"));
    }

    #[test]
    fn counterparty_cannot_select_owner_route_persona() {
        let mut personas = HashMap::new();
        personas.insert("owner-facing".to_string(), persona("owner-facing"));
        // The resolver consumes only the already-winning route. A
        // counterparty that does not match that route supplies no route here,
        // so it receives no persona rather than inheriting the owner's.
        let selected = resolve_persona(
            "counterparty-number",
            Some(RelationshipKind::Unknown),
            &route(None),
            &personas,
        );
        assert_eq!(selected, None);
    }

    #[test]
    fn route_context_mismatch_cannot_front_persona() {
        let mut personas = HashMap::new();
        personas.insert("owner-facing".to_string(), persona("owner-facing"));
        let mut owner_route = route(Some("owner-facing"));
        owner_route.when = RouteWhen {
            channel_account: Some("owner-number".to_string()),
            actor: Some(RouteActorWhen {
                relationship: Some(RelationshipKind::Owner),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert_eq!(
            resolve_persona(
                "other-number",
                Some(RelationshipKind::Owner),
                &owner_route,
                &personas,
            ),
            None,
            "a route-bound persona cannot cross channel accounts"
        );
        assert_eq!(
            resolve_persona(
                "owner-number",
                Some(RelationshipKind::Unknown),
                &owner_route,
                &personas,
            ),
            None,
            "a route-bound persona cannot cross relationships"
        );
    }
}
