mod tests {
    use crate::connector_reality::BreakerState;
    use jiff::Timestamp;
    use openspine_gate::{gate, ActionOrigin, GateContext, NoEgress};
    use openspine_schemas::action::{
        ActionCatalog, ActionEgressDeclaration, ActionId, ActionRequest, DenialReason, GateDecision,
    };
    use openspine_schemas::approval::ApprovalRecord;
    use openspine_schemas::artifact::Lifecycle;
    use openspine_schemas::egress::EgressClass;
    use openspine_schemas::grant::{GrantLimits, GrantMode, TaskGrant};
    use openspine_schemas::selection::SelectionToken;
    use ulid::Ulid;

    use crate::gmail::GmailConnector;
    use crate::telegram::TelegramConnector;

    use super::super::{Connector, ConnectorRegistry};

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
        ])
        .with_egress_declarations([
            (
                ActionId::new("web.search"),
                ActionEgressDeclaration {
                    output_channels: None,
                    egress_class: Some(EgressClass::Search),
                },
            ),
            (
                ActionId::new("web.form_submit"),
                ActionEgressDeclaration {
                    output_channels: None,
                    egress_class: Some(EgressClass::WebFormPost),
                },
            ),
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
            params: std::collections::BTreeMap::new(),
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
            params: std::collections::BTreeMap::new(),
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
        let catalog = ActionCatalog::new([ActionId::new("openspine.status.read")])
            .with_egress_declarations([(
                ActionId::new("openspine.status.read"),
                ActionEgressDeclaration {
                    output_channels: None,
                    egress_class: None,
                },
            )]);
        let req = ActionRequest {
            id: Ulid::new(),
            task_grant_id: grant.id,
            action: ActionId::new("openspine.status.read"),
            target_ref: None,
            payload_ref: None,
            target_digest: None,
            selection_token_id: None,
            params: std::collections::BTreeMap::new(),
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
    #[test]
    fn registry_rate_buckets_are_isolated_per_connector() {
        let registry =
            ConnectorRegistry::new(TelegramConnector::new("t".to_string()), Some(gmail()))
                .expect("built-in egress ratings are conflict-free");
        for _ in 0..10 {
            assert!(registry.acquire_connector("telegram").is_ok());
        }
        assert!(matches!(
            registry.acquire_connector("telegram"),
            Err(crate::connector_reality::ConnectorCallError::RateLimited { .. })
        ));
        assert!(registry.acquire_connector("gmail").is_ok());
    }
    #[test]
    fn gmail_failure_records_breaker_failure_and_opens_breaker() {
        // R2: a connector failure (e.g. a timed-out call, whose path calls
        // `record_connector_outcome(connector, false)`) is recorded into the
        // breaker; after the failure threshold the breaker opens, and a later
        // success closes it again.
        let registry =
            ConnectorRegistry::new(TelegramConnector::new("t".to_string()), Some(gmail()))
                .expect("built-in egress ratings are conflict-free");
        for _ in 0..3 {
            registry.record_connector_outcome("gmail", false);
        }
        assert!(matches!(
            registry.breaker_state("gmail"),
            Some(BreakerState::Open { .. })
        ));
        registry.record_connector_outcome("gmail", true);
        assert_eq!(registry.breaker_state("gmail"), Some(BreakerState::Closed));
    }
}
