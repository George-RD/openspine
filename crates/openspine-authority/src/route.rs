//! Deterministic route resolution (PRD §6.2/§6.3/§6.4).
//!
//! LLMs may not resolve route conflicts that affect authority (PRD §6.2
//! rule 5) — this function is the entire, deterministic conflict-resolution
//! algorithm. No semantic/LLM-based specificity scoring is allowed.

use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::event::EventEnvelope;
use openspine_schemas::identity::{IdentityResolution, RelationshipKind};
use openspine_schemas::route::{Route, RouteResolution};

const LOW_AUTHORITY_TRIAGE: &str = "low_authority_triage";

/// Does `route.when` match this event/identity exactly?
///
/// Every explicit `when` field must match; absent fields impose no
/// constraint. `relationship` cannot be read off [`IdentityResolution`]
/// alone (PRD §5.4 carries no relationship field) — the caller supplies it
/// separately, resolved from the matched identity's relationship graph, so
/// this pure function never needs to look anything up itself.
fn matches(
    event: &EventEnvelope,
    identity: &IdentityResolution,
    relationship: Option<RelationshipKind>,
    route: &Route,
) -> bool {
    let when = &route.when;

    if let Some(source) = when.source {
        if event.source != source {
            return false;
        }
    }
    if let Some(event_type) = when.event_type {
        if event.event_type != event_type {
            return false;
        }
    }
    if let Some(verified) = when.verified_source {
        if event.verified_source != verified {
            return false;
        }
    }
    if let Some(lane) = when.lane {
        if event.lane != lane {
            return false;
        }
    }
    if let Some(connector) = when.connector {
        if event.connector != Some(connector) {
            return false;
        }
    }
    if let Some(account_role) = when.account_role {
        if event.account_role != Some(account_role) {
            return false;
        }
    }
    if let Some(wanted) = &when.channel_account {
        if event.channel_account != *wanted {
            return false;
        }
    }
    if let Some(actor) = &when.actor {
        if let Some(wanted) = actor.relationship {
            if relationship != Some(wanted) {
                return false;
            }
        }
        if let Some(wanted) = actor.channel_trust {
            if identity.channel_trust != wanted {
                return false;
            }
        }
        if let Some(min) = actor.identity_confidence_min {
            if identity.confidence < min {
                return false;
            }
        }
    }
    true
}

/// Resolve the single winning route for an event, or report why none won
/// (PRD §6.2/§6.4). Only `active` routes are considered (PRD §13.2:
/// quarantined artifacts cannot participate in task grants).
pub fn resolve_route(
    event: &EventEnvelope,
    identity: &IdentityResolution,
    relationship: Option<RelationshipKind>,
    routes: &[Route],
) -> RouteResolution {
    let candidates: Vec<&Route> = routes
        .iter()
        .filter(|r| r.lifecycle_state == Lifecycle::Active)
        .filter(|r| matches(event, identity, relationship, r))
        .collect();

    if candidates.is_empty() {
        return RouteResolution::Denied {
            reason: "no active route matched the event".to_string(),
        };
    }

    // Rule 1: exact deny route wins over allow route.
    if let Some(deny) = candidates
        .iter()
        .find(|r| r.effect == openspine_schemas::route::RouteEffect::Deny)
    {
        return RouteResolution::Denied {
            reason: format!("route {} explicitly denies this event", deny.id),
        };
    }

    // Rules 2/3: highest explicit priority wins; ties broken by specificity;
    // remaining ties are ambiguous (rule 4).
    let max_priority = candidates.iter().filter_map(|r| r.priority).max();
    let by_priority: Vec<&&Route> = match max_priority {
        Some(p) => candidates
            .iter()
            .filter(|r| r.priority == Some(p))
            .collect(),
        None => candidates.iter().collect(),
    };

    let max_specificity = by_priority
        .iter()
        .map(|r| r.when.specificity())
        .max()
        .unwrap_or(0);
    let winners: Vec<&&Route> = by_priority
        .iter()
        .filter(|r| r.when.specificity() == max_specificity)
        .copied()
        .collect();

    match winners.as_slice() {
        [only] => RouteResolution::Success {
            route_id: only.id.clone(),
        },
        _ => RouteResolution::Ambiguous {
            fallback_route: LOW_AUTHORITY_TRIAGE.to_string(),
            reason: "multiple_matching_routes_no_deterministic_winner".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspine_schemas::artifact::ArtifactRef;
    use openspine_schemas::digest::Digest;
    use openspine_schemas::event::{
        AccountRole, ActorHint, ChannelTrust, Connector, DataClassification, EventType,
        InteractionMode, Lane, Source, TrustContext, VerificationMethod,
    };
    use openspine_schemas::route::{RouteActorWhen, RouteEffect, RouteWhen};
    use ulid::Ulid;

    fn artifact_ref() -> ArtifactRef {
        ArtifactRef {
            digest: Digest::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
            schema_version: 1,
        }
    }

    fn owner_event() -> EventEnvelope {
        EventEnvelope {
            id: Ulid::new(),
            source: Source::Telegram,
            connector: Some(Connector::TelegramOwnerBot),
            account_role: Some(AccountRole::OwnerControlAccount),
            event_type: EventType::TelegramOwnerMessage,
            received_at: jiff::Timestamp::now(),
            verified_source: true,
            verification_method: VerificationMethod::TelegramOwnerIdMatch,
            replay_protected: true,
            replay_nonce: None,
            channel_account: "123".to_string(),
            raw_event_ref: artifact_ref(),
            actor_hint: ActorHint::default(),
            target_refs: vec![],
            data_classification: DataClassification::Private,
            user_intent_hint: None,
            lane: Lane::OwnerControl,
            trust_context: TrustContext {
                channel_trust: ChannelTrust::VerifiedOwnerChannel,
                interaction_mode: InteractionMode::OwnerMessage,
            },
            thread_id: None,
            schema_version: 1,
        }
    }

    fn owner_identity_resolution() -> IdentityResolution {
        IdentityResolution {
            event_id: Ulid::new(),
            matched_identity_id: Some(Ulid::new()),
            principal_id: Some(Ulid::new()),
            confidence: 1.0,
            matched_identifier_type:
                openspine_schemas::identity::MatchedIdentifierType::TelegramUserId,
            channel_trust: ChannelTrust::VerifiedOwnerChannel,
            source_verified: true,
            authority_warning: None,
            schema_version: 1,
        }
    }

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
                    channel_trust: Some(ChannelTrust::VerifiedOwnerChannel),
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
    fn owner_route_matches_when_verified_and_owner() {
        let resolution = resolve_route(
            &owner_event(),
            &owner_identity_resolution(),
            Some(RelationshipKind::Owner),
            &[owner_route()],
        );
        assert_eq!(
            resolution,
            RouteResolution::Success {
                route_id: "owner_telegram_main_assistant".to_string()
            }
        );
    }

    #[test]
    fn unverified_source_never_matches_a_route_requiring_verification() {
        let mut event = owner_event();
        event.verified_source = false;
        let resolution = resolve_route(
            &event,
            &owner_identity_resolution(),
            Some(RelationshipKind::Owner),
            &[owner_route()],
        );
        assert!(matches!(resolution, RouteResolution::Denied { .. }));
    }

    fn email_route() -> Route {
        let mut route = owner_route();
        route.id = "owner_email_selected_thread".to_string();
        route.priority = Some(90);
        route.when.source = Some(Source::Gmail);
        route.when.event_type = Some(EventType::EmailThreadSelected);
        route.when.lane = Some(Lane::ExternalCommunication);
        route.when.connector = Some(Connector::GmailPrimaryConnector);
        route.when.account_role = Some(AccountRole::OwnerMailbox);
        route.when.actor.as_mut().unwrap().channel_trust = Some(ChannelTrust::OwnerDevice);
        route.agent = Some("email_reply_drafter".to_string());
        route.workflow = Some("selected_thread_email_reply_draft".to_string());
        route.capability_pack = Some("selected_thread_email_draft_pack".to_string());
        route
    }

    #[test]
    fn gmail_connector_authenticated_alone_does_not_match_the_selected_thread_route() {
        // spec.md: "Given the Gmail connector is authenticated And no
        // selected-thread token exists When authority composition runs
        // Then selected-thread read authority MUST NOT be granted." At the
        // routing layer this means: an event that is Gmail-connector- and
        // account-role-authenticated but is NOT the specific
        // `email.thread.selected` event produced by a real selection flow
        // must not match `owner_email_selected_thread` at all.
        let mut event = owner_event();
        event.source = Source::Gmail;
        event.connector = Some(Connector::GmailPrimaryConnector);
        event.account_role = Some(AccountRole::OwnerMailbox);
        event.lane = Lane::ExternalCommunication;
        event.trust_context.channel_trust = ChannelTrust::OwnerDevice;
        // event_type deliberately left as TelegramOwnerMessage — connector
        // authentication alone, without the selection event, is not enough.

        let resolution = resolve_route(
            &event,
            &owner_identity_resolution(),
            Some(RelationshipKind::Owner),
            &[email_route()],
        );
        assert!(matches!(resolution, RouteResolution::Denied { .. }), "connector/account-role alone must not match the selected-thread route, got {resolution:?}");
    }

    #[test]
    fn no_relationship_match_denies_the_route() {
        let resolution = resolve_route(
            &owner_event(),
            &owner_identity_resolution(),
            None,
            &[owner_route()],
        );
        assert!(matches!(resolution, RouteResolution::Denied { .. }));
    }

    #[test]
    fn exact_deny_route_wins_over_allow_route() {
        let mut deny_route = owner_route();
        deny_route.id = "deny_owner".to_string();
        deny_route.effect = RouteEffect::Deny;
        deny_route.priority = Some(50); // lower priority than the allow route, but deny always wins.

        let resolution = resolve_route(
            &owner_event(),
            &owner_identity_resolution(),
            Some(RelationshipKind::Owner),
            &[owner_route(), deny_route],
        );
        assert!(matches!(resolution, RouteResolution::Denied { .. }));
    }

    #[test]
    fn priority_tie_with_equal_specificity_is_ambiguous() {
        let mut second = owner_route();
        second.id = "owner_telegram_main_assistant_v2".to_string();
        let resolution = resolve_route(
            &owner_event(),
            &owner_identity_resolution(),
            Some(RelationshipKind::Owner),
            &[owner_route(), second],
        );
        assert_eq!(
            resolution,
            RouteResolution::Ambiguous {
                fallback_route: LOW_AUTHORITY_TRIAGE.to_string(),
                reason: "multiple_matching_routes_no_deterministic_winner".to_string()
            }
        );
    }

    #[test]
    fn higher_priority_route_wins_over_lower_priority() {
        let mut low = owner_route();
        low.id = "low_priority".to_string();
        low.priority = Some(1);

        let resolution = resolve_route(
            &owner_event(),
            &owner_identity_resolution(),
            Some(RelationshipKind::Owner),
            &[low, owner_route()],
        );
        assert_eq!(
            resolution,
            RouteResolution::Success {
                route_id: "owner_telegram_main_assistant".to_string()
            }
        );
    }

    #[test]
    fn quarantined_routes_never_match() {
        let mut route = owner_route();
        route.lifecycle_state = Lifecycle::Quarantined;
        let resolution = resolve_route(
            &owner_event(),
            &owner_identity_resolution(),
            Some(RelationshipKind::Owner),
            &[route],
        );
        assert!(matches!(resolution, RouteResolution::Denied { .. }));
    }

    #[test]
    fn no_matching_route_is_denied_not_ambiguous() {
        let resolution = resolve_route(
            &owner_event(),
            &owner_identity_resolution(),
            Some(RelationshipKind::Owner),
            &[],
        );
        assert_eq!(
            resolution,
            RouteResolution::Denied {
                reason: "no active route matched the event".to_string()
            }
        );
    }
}
