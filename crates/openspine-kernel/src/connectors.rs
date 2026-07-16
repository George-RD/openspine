//! Connector registry (kernel registry refactor, part 2 + AD-060).
//!
//! Connectors are held in a single registry that is the one registration
//! point for connector instances. Gmail's optionality is preserved
//! bit-for-bit: the registry reports it absent when unconfigured so call
//! sites keep their graceful-degradation branches (see
//! `pipeline::driver::email_preview_lane` and
//! `pipeline::approval::create_approved_draft`).
//!
//! AD-060: the registry is also the source of truth for egress-endpoint
//! ratings. Endpoint → class mappings live here — not on the request —
//! and the gate queries them through [`EgressClassifier`].

use std::collections::{hash_map::Entry, HashMap};

use openspine_gate::EgressClassifier;
use openspine_schemas::action::ActionId;
use openspine_schemas::egress::EgressClass;

use crate::gmail::GmailConnector;
use crate::telegram::TelegramConnector;

/// A kernel connector. The trait is the AD-060 / AD-103 registration
/// seam: connectors declare their name and any rated egress endpoints.
pub trait Connector {
    #[allow(dead_code)] // the AD-060/AD-103 enumeration seam; exercised via `iter()` in tests today
    fn name(&self) -> &'static str;

    /// AD-060: egress endpoints this connector exposes, each rated with
    /// its egress class. Connectors with no rated egress return empty.
    fn egress_endpoints(&self) -> Vec<(ActionId, EgressClass)> {
        Vec::new()
    }
}

impl Connector for TelegramConnector {
    fn name(&self) -> &'static str {
        "telegram"
    }
}

impl Connector for GmailConnector {
    fn name(&self) -> &'static str {
        "gmail"
    }
}

/// Built-in AD-060 web-egress endpoints rated in the connector registry.
/// These represent the web egress surface packs authorize by class; a
/// future web connector will dispatch them, but the rating lives here so
/// the gate can enforce class coverage before any dispatcher exists.
fn built_in_web_egress_endpoints() -> Vec<(ActionId, EgressClass)> {
    vec![
        (ActionId::new("web.search"), EgressClass::Search),
        (ActionId::new("web.forum_browse"), EgressClass::ForumBrowse),
        (ActionId::new("web.form_submit"), EgressClass::WebFormPost),
    ]
}

/// A conflicting endpoint rating is rejected rather than silently changing
/// the policy class of an already-registered endpoint (AD-060).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressRegistrationError {
    pub action: ActionId,
    pub existing: EgressClass,
    pub requested: EgressClass,
}

impl std::fmt::Display for EgressRegistrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "egress endpoint {} already rated {:?}, cannot register {:?}",
            self.action, self.existing, self.requested
        )
    }
}

impl std::error::Error for EgressRegistrationError {}

/// The kernel's connector registry: the single registration point for
/// connector instances and for egress-endpoint class ratings (AD-060).
/// `telegram` is always present (the long-poll loop depends on it);
/// `gmail` is optional and reported as absent when unconfigured.
pub struct ConnectorRegistry {
    telegram: TelegramConnector,
    gmail: Option<GmailConnector>,
    /// Aggregated endpoint → class ratings from built-in web egress plus
    /// every configured connector's `egress_endpoints()`.
    egress_ratings: HashMap<ActionId, EgressClass>,
}
impl ConnectorRegistry {
    pub fn new(
        telegram: TelegramConnector,
        gmail: Option<GmailConnector>,
    ) -> Result<Self, EgressRegistrationError> {
        let mut declared = telegram.egress_endpoints();
        if let Some(gmail) = &gmail {
            declared.extend(gmail.egress_endpoints());
        }
        let egress_ratings = Self::build_egress_ratings(declared)?;
        Ok(Self {
            telegram,
            gmail,
            egress_ratings,
        })
    }

    fn build_egress_ratings<I>(
        declared: I,
    ) -> Result<HashMap<ActionId, EgressClass>, EgressRegistrationError>
    where
        I: IntoIterator<Item = (ActionId, EgressClass)>,
    {
        let mut ratings = HashMap::new();
        for (action, class) in built_in_web_egress_endpoints() {
            Self::insert_rating(&mut ratings, action, class)?;
        }
        for (action, class) in declared {
            Self::insert_rating(&mut ratings, action, class)?;
        }
        Ok(ratings)
    }

    fn insert_rating(
        ratings: &mut HashMap<ActionId, EgressClass>,
        action: ActionId,
        class: EgressClass,
    ) -> Result<(), EgressRegistrationError> {
        match ratings.entry(action) {
            Entry::Vacant(slot) => {
                slot.insert(class);
                Ok(())
            }
            Entry::Occupied(existing) if *existing.get() == class => Ok(()),
            Entry::Occupied(existing) => Err(EgressRegistrationError {
                action: existing.key().clone(),
                existing: *existing.get(),
                requested: class,
            }),
        }
    }

    /// The Telegram connector is always configured; the long-poll loop
    /// depends on it.
    pub fn telegram(&self) -> &TelegramConnector {
        &self.telegram
    }

    /// `None` when Gmail isn't configured — call sites use this to degrade
    /// gracefully (draft creation, `/draft` selection).
    pub fn gmail(&self) -> Option<&GmailConnector> {
        self.gmail.as_ref()
    }

    /// AD-060: look up the egress class for a rated endpoint. `None` means
    /// the action is not a rated egress endpoint (gate skips the check).
    pub fn egress_class_for(&self, action: &ActionId) -> Option<EgressClass> {
        self.egress_ratings.get(action).copied()
    }

    /// Enumerate every configured connector with its registered name.
    #[allow(dead_code)] // the AD-060/AD-103 enumeration seam; production callers arrive with connector health/egress typing
    pub fn iter(&self) -> impl Iterator<Item = &dyn Connector> {
        let mut v: Vec<&dyn Connector> = Vec::with_capacity(2);
        v.push(&self.telegram);
        if let Some(gmail) = &self.gmail {
            v.push(gmail);
        }
        v.into_iter()
    }
}

impl EgressClassifier for ConnectorRegistry {
    fn classify(&self, action: &ActionId) -> Option<EgressClass> {
        self.egress_class_for(action)
    }
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use openspine_gate::{gate, ActionOrigin, GateContext, NoEgress};
    use openspine_schemas::action::{
        ActionCatalog, ActionId, ActionRequest, DenialReason, GateDecision,
    };
    use openspine_schemas::approval::ApprovalRecord;
    use openspine_schemas::artifact::Lifecycle;
    use openspine_schemas::egress::EgressClass;
    use openspine_schemas::grant::{GrantLimits, GrantMode, TaskGrant};
    use openspine_schemas::selection::SelectionToken;
    use ulid::Ulid;

    use crate::gmail::GmailConnector;
    use crate::telegram::TelegramConnector;

    use super::{Connector, ConnectorRegistry};

    fn gmail() -> GmailConnector {
        GmailConnector::new(
            "cid".to_string(),
            "csec".to_string(),
            "rtok".to_string(),
            "owner@example.com".to_string(),
        )
    }

    #[test]
    fn connector_registry_enumerates_configured_connectors() {
        // Gmail absent.
        let absent = ConnectorRegistry::new(TelegramConnector::new("t".to_string()), None)
            .expect("built-in egress ratings are conflict-free");
        let names: Vec<&str> = absent.iter().map(Connector::name).collect();
        assert_eq!(names, vec!["telegram"]);

        // Gmail present.
        let present =
            ConnectorRegistry::new(TelegramConnector::new("t".to_string()), Some(gmail()))
                .expect("built-in egress ratings are conflict-free");
        let names: Vec<&str> = present.iter().map(Connector::name).collect();
        assert_eq!(names, vec!["telegram", "gmail"]);

        // Accessors reflect configuration.
        assert!(present.gmail().is_some());
        assert!(absent.gmail().is_none());
        assert_eq!(present.telegram().name(), "telegram");
    }

    #[test]
    fn registry_rates_built_in_web_egress_endpoints() {
        let registry = ConnectorRegistry::new(TelegramConnector::new("t".to_string()), None)
            .expect("built-in egress ratings are conflict-free");
        assert_eq!(
            registry.egress_class_for(&ActionId::new("web.search")),
            Some(EgressClass::Search)
        );
        assert_eq!(
            registry.egress_class_for(&ActionId::new("web.forum_browse")),
            Some(EgressClass::ForumBrowse)
        );
        assert_eq!(
            registry.egress_class_for(&ActionId::new("web.form_submit")),
            Some(EgressClass::WebFormPost)
        );
        // Unrated action is not an egress endpoint.
        assert_eq!(
            registry.egress_class_for(&ActionId::new("openspine.status.read")),
            None
        );
    }

    #[test]
    fn conflicting_egress_rating_is_rejected_without_downgrade() {
        let action = ActionId::new("web.form_submit");
        let error =
            ConnectorRegistry::build_egress_ratings(vec![(action.clone(), EgressClass::Search)])
                .expect_err("conflicting class must be rejected");
        assert_eq!(error.action, action);
        assert_eq!(error.existing, EgressClass::WebFormPost);
        assert_eq!(error.requested, EgressClass::Search);

        // Same-class declarations are idempotent in the constructor path.
        let ratings = ConnectorRegistry::build_egress_ratings(vec![(
            ActionId::new("web.form_submit"),
            EgressClass::WebFormPost,
        )])
        .expect("same class is idempotent");
        assert_eq!(
            ratings.get(&ActionId::new("web.form_submit")),
            Some(&EgressClass::WebFormPost)
        );
    }

    /// AD-060 Done-when: a pack granted search-class egress cannot submit
    /// a web form. Exercises registry → gate end-to-end (no mock classifier).
    #[test]
    fn search_class_pack_cannot_submit_web_form() {
        let registry = ConnectorRegistry::new(TelegramConnector::new("t".to_string()), None)
            .expect("built-in egress ratings are conflict-free");

        // Prove the registry itself rates form-submit as WebFormPost.
        assert_eq!(
            registry.egress_class_for(&ActionId::new("web.form_submit")),
            Some(EgressClass::WebFormPost)
        );

        let issued_at = Timestamp::now();
        let mut grant = TaskGrant {
            id: Ulid::new(),
            schema_version: 1,
            lifecycle_state: Lifecycle::Active,
            user: "owner".to_string(),
            purpose: "test".to_string(),
            issued_by: "kernel".to_string(),
            issued_at,
            expires_at: issued_at + std::time::Duration::from_secs(120),
            event_id: Ulid::new(),
            route_id: "owner_telegram_main_assistant".to_string(),
            agent_id: "main_assistant_agent".to_string(),
            workflow_id: "owner_control_conversation".to_string(),
            capability_pack_id: "search_only_pack".to_string(),
            authority_sources: vec![],
            selection_tokens: vec![],
            // Both actions are action-allowed; egress class is the finer constraint.
            allowed_actions: vec![
                ActionId::new("web.search"),
                ActionId::new("web.form_submit"),
            ],
            approval_required_actions: vec![],
            denied_actions: vec![],
            allowed_egress_classes: vec![EgressClass::Search],
            output_channels: vec![],
            limits: GrantLimits {
                max_model_calls: 8,
                max_artifacts: 20,
                max_runtime_seconds: 120,
            },
            task_token: "a".repeat(64),
            root_grant_id: Ulid::nil(),
            parent_grant_id: None,
            mode: GrantMode::Live,
            chain: vec![],
            caveat_mac: String::new(),
            thread_id: None,
        };
        grant.seal_root(b"openspine-test-grant-hmac-key-v1");

        let catalog = ActionCatalog::new([
            ActionId::new("web.search"),
            ActionId::new("web.form_submit"),
        ]);
        let ctx = EmptyGateContext;
        let now = Timestamp::now();

        // Form submit is denied: registry rates it WebFormPost, grant only has Search.
        let form_req = ActionRequest {
            id: Ulid::new(),
            task_grant_id: grant.id,
            action: ActionId::new("web.form_submit"),
            target_ref: None,
            payload_ref: None,
            target_digest: None,
            selection_token_id: None,
            requested_at: now,
            schema_version: 1,
        };
        let outcome = gate(
            &grant,
            &form_req,
            ActionOrigin::Shell,
            &ctx,
            &catalog,
            &registry, // real ConnectorRegistry, not a mock classifier
            now,
        );
        assert_eq!(
            outcome.decision,
            GateDecision::Deny {
                reason: DenialReason::EgressClassNotGranted
            }
        );

        // Search is allowed: same grant, registry rates it Search.
        let search_req = ActionRequest {
            id: Ulid::new(),
            task_grant_id: grant.id,
            action: ActionId::new("web.search"),
            target_ref: None,
            payload_ref: None,
            target_digest: None,
            selection_token_id: None,
            requested_at: now,
            schema_version: 1,
        };
        let outcome = gate(
            &grant,
            &search_req,
            ActionOrigin::Shell,
            &ctx,
            &catalog,
            &registry,
            now,
        );
        assert_eq!(outcome.decision, GateDecision::Allow);
    }

    /// GateContext with no approvals/tokens — only used for the egress test.
    struct EmptyGateContext;

    impl GateContext for EmptyGateContext {
        fn approval_for_request(&self, _action_request_id: Ulid) -> Option<ApprovalRecord> {
            None
        }

        fn find_selection_token(&self, _id: Ulid) -> Option<SelectionToken> {
            None
        }

        fn grant_hmac_key(&self) -> Option<Vec<u8>> {
            Some(b"openspine-test-grant-hmac-key-v1".to_vec())
        }
    }

    #[test]
    fn no_egress_classifier_skips_check_for_unrated_actions() {
        // Sanity: NoEgress still lets unrated actions through on allow-list.
        let issued_at = Timestamp::now();
        let mut grant = TaskGrant {
            id: Ulid::new(),
            schema_version: 1,
            lifecycle_state: Lifecycle::Active,
            user: "owner".to_string(),
            purpose: "test".to_string(),
            issued_by: "kernel".to_string(),
            issued_at,
            expires_at: issued_at + std::time::Duration::from_secs(120),
            event_id: Ulid::new(),
            route_id: "r".to_string(),
            agent_id: "a".to_string(),
            workflow_id: "w".to_string(),
            capability_pack_id: "p".to_string(),
            authority_sources: vec![],
            selection_tokens: vec![],
            allowed_actions: vec![ActionId::new("openspine.status.read")],
            approval_required_actions: vec![],
            denied_actions: vec![],
            allowed_egress_classes: vec![],
            output_channels: vec![],
            limits: GrantLimits {
                max_model_calls: 8,
                max_artifacts: 20,
                max_runtime_seconds: 120,
            },
            task_token: "a".repeat(64),
            root_grant_id: Ulid::nil(),
            parent_grant_id: None,
            mode: GrantMode::Live,
            chain: vec![],
            caveat_mac: String::new(),
            thread_id: None,
        };
        grant.seal_root(b"openspine-test-grant-hmac-key-v1");
        let catalog = ActionCatalog::new([ActionId::new("openspine.status.read")]);
        let req = ActionRequest {
            id: Ulid::new(),
            task_grant_id: grant.id,
            action: ActionId::new("openspine.status.read"),
            target_ref: None,
            payload_ref: None,
            target_digest: None,
            selection_token_id: None,
            requested_at: Timestamp::now(),
            schema_version: 1,
        };
        let outcome = gate(
            &grant,
            &req,
            ActionOrigin::Shell,
            &EmptyGateContext,
            &catalog,
            &NoEgress,
            Timestamp::now(),
        );
        assert_eq!(outcome.decision, GateDecision::Allow);
    }
}
