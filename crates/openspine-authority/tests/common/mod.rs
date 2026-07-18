//! Shared fixture builders for `openspine-authority` integration tests.
//! Split out of `tests/compose.rs` to keep each file under the
//! 500-line-per-file gate — these are plain data builders, not tests.

use openspine_authority::AuthorityInput;
use openspine_schemas::action::{ActionCatalog, ActionId};
use openspine_schemas::agent::{
    AgentLimits, AgentManifest, ModelPolicy, OutputChannels, Persistence,
};
use openspine_schemas::artifact::{ArtifactRef, Lifecycle};
use openspine_schemas::digest::Digest;
use openspine_schemas::event::{
    AccountRole, ActorHint, ChannelTrust, Connector, DataClassification, EventEnvelope, EventType,
    InteractionMode, Lane, Source, TargetRef, TargetRefKind, TrustContext, VerificationMethod,
};
use openspine_schemas::identity::{IdentityResolution, MatchedIdentifierType, RelationshipKind};
use openspine_schemas::model::Provider;
use openspine_schemas::pack::{AppliesTo, CapabilityPack};
use openspine_schemas::policy::{Constraints, Policy, SessionPolicy};
use openspine_schemas::route::{Route, RouteActorWhen, RouteEffect, RouteWhen};
use openspine_schemas::workflow::WorkflowManifest;
use ulid::Ulid;

pub fn artifact_ref() -> ArtifactRef {
    ArtifactRef {
        digest: Digest::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
        schema_version: 1,
    }
}

pub fn owner_event() -> EventEnvelope {
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

pub fn email_event() -> EventEnvelope {
    EventEnvelope {
        id: Ulid::new(),
        source: Source::Gmail,
        connector: Some(Connector::GmailPrimaryConnector),
        account_role: Some(AccountRole::OwnerMailbox),
        event_type: EventType::EmailThreadSelected,
        received_at: jiff::Timestamp::now(),
        verified_source: true,
        verification_method: VerificationMethod::KernelUiSelection,
        replay_protected: true,
        replay_nonce: None,
        channel_account: "owner@example.com".to_string(),
        raw_event_ref: artifact_ref(),
        actor_hint: ActorHint::default(),
        target_refs: vec![TargetRef {
            kind: TargetRefKind::EmailThread,
            id: Some("thread_1".to_string()),
        }],
        data_classification: DataClassification::Private,
        user_intent_hint: None,
        lane: Lane::ExternalCommunication,
        trust_context: TrustContext {
            channel_trust: ChannelTrust::OwnerDevice,
            interaction_mode: InteractionMode::UserSelected,
        },
        thread_id: None,
        schema_version: 1,
    }
}

pub fn owner_identity() -> IdentityResolution {
    IdentityResolution {
        event_id: Ulid::new(),
        matched_identity_id: Some(Ulid::new()),
        principal_id: Some(Ulid::new()),
        confidence: 1.0,
        matched_identifier_type: MatchedIdentifierType::TelegramUserId,
        channel_trust: ChannelTrust::VerifiedOwnerChannel,
        source_verified: true,
        authority_warning: None,
        schema_version: 1,
    }
}

pub fn owner_route() -> Route {
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

pub fn email_route() -> Route {
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

pub fn main_assistant_agent() -> AgentManifest {
    AgentManifest {
        id: "main_assistant_agent".to_string(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        purpose: "Owner-facing conversational orchestrator.".to_string(),
        persistence: Persistence::Persistent,
        persona: "concise_practical_operator".to_string(),
        model_policy: ModelPolicy {
            allowed_providers: vec![Provider::Local, Provider::Openai, Provider::Anthropic],
            private_context_requires_gateway: true,
            max_model_calls_per_task: 8,
        },
        memory_scope: Default::default(),
        designed_tools: vec![
            ActionId::new("openspine.status.read"),
            ActionId::new("workflow.invoke:approved"),
            ActionId::new("artifact.propose"),
            ActionId::new("artifact.nominate_upstream"),
            ActionId::new("setup.workflow.start"),
            ActionId::new("memory.read:owner_preferences_limited"),
            ActionId::new("model.generate:approved_provider"),
            ActionId::new("lyra.ui.preview"),
            ActionId::new("telegram.reply:owner_channel"),
        ],
        approval_required_tools: vec![
            ActionId::new("connector.enable"),
            ActionId::new("route.activate"),
            ActionId::new("capability_pack.change"),
            ActionId::new("workflow.activate"),
            ActionId::new("policy.change_proposal"),
        ],
        denied_tools: vec![
            ActionId::new("email.read_inbox"),
            ActionId::new("email.read_thread:unselected"),
            ActionId::new("email.send"),
            ActionId::new("email.read_attachment"),
            ActionId::new("network.raw_egress"),
            ActionId::new("vault.secret_read"),
            ActionId::new("policy.modify_direct"),
            ActionId::new("filesystem.host_read"),
            ActionId::new("filesystem.host_write"),
            ActionId::new("coolify.deploy"),
            ActionId::new("coolify.rollback"),
            ActionId::new("coolify.secret_modify"),
        ],
        limits: AgentLimits {
            max_runtime_seconds: 120,
            max_artifacts: 20,
            max_tokens: 12_000,
        },
        output_channels: OutputChannels {
            allowed: vec![
                "telegram.owner.reply".to_string(),
                "lyra.ui.preview".to_string(),
                "action_request:approval".to_string(),
            ],
        },
    }
}

pub fn email_reply_drafter_agent() -> AgentManifest {
    AgentManifest {
        id: "email_reply_drafter".to_string(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        purpose: "Draft replies to selected email threads for user review.".to_string(),
        persistence: Persistence::Ephemeral,
        persona: "concise_professional_helper".to_string(),
        model_policy: ModelPolicy {
            allowed_providers: vec![Provider::Local, Provider::Openai, Provider::Anthropic],
            private_context_requires_gateway: true,
            max_model_calls_per_task: 5,
        },
        memory_scope: Default::default(),
        // D-034: bare `email.create_draft`, not PRD §10.2's qualified spelling.
        designed_tools: vec![
            ActionId::new("email.read_thread:selected_no_attachments"),
            ActionId::new("model.generate:approved_provider"),
            ActionId::new("memory.read:writing_preferences_scoped"),
            ActionId::new("artifact.write:task_scratch"),
            ActionId::new("lyra.ui.preview"),
            ActionId::new("email.create_draft"),
        ],
        approval_required_tools: vec![],
        denied_tools: vec![
            ActionId::new("email.send"),
            ActionId::new("email.read_inbox"),
            ActionId::new("email.read_thread:unselected"),
            ActionId::new("email.read_attachment"),
            ActionId::new("network.raw_egress"),
            ActionId::new("telegram.reply:owner_channel"),
        ],
        limits: AgentLimits {
            max_runtime_seconds: 180,
            max_artifacts: 20,
            max_tokens: 12_000,
        },
        output_channels: OutputChannels {
            allowed: vec![
                "lyra.ui.preview".to_string(),
                "action_request:email.create_draft".to_string(),
            ],
        },
    }
}

pub fn owner_control_conversation_workflow() -> WorkflowManifest {
    WorkflowManifest {
        id: "owner_control_conversation".to_string(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        purpose: "Owner-facing conversational orchestration.".to_string(),
        required_agent: "main_assistant_agent".to_string(),
        required_capability_pack: "owner_control_basic_pack".to_string(),
        steps: vec![],
        candidate_allowed_actions: vec![
            ActionId::new("openspine.status.read"),
            ActionId::new("telegram.reply:owner_channel"),
        ],
        approval_required: vec![],
        denied_actions: vec![],
        initial_state: None,
        states: vec![],
        transitions: vec![],
    }
}

pub fn selected_thread_email_reply_draft_workflow() -> WorkflowManifest {
    WorkflowManifest {
        id: "selected_thread_email_reply_draft".to_string(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        purpose: "Draft a reply to an owner-selected Gmail thread for review, with no send."
            .to_string(),
        required_agent: "email_reply_drafter".to_string(),
        required_capability_pack: "selected_thread_email_draft_pack".to_string(),
        steps: vec![],
        candidate_allowed_actions: vec![
            ActionId::new("email.read_thread:selected_no_attachments"),
            ActionId::new("model.generate:approved_provider"),
        ],
        approval_required: vec![ActionId::new("email.create_draft")],
        denied_actions: vec![],
        initial_state: None,
        states: vec![],
        transitions: vec![],
    }
}

pub fn owner_control_basic_pack() -> CapabilityPack {
    CapabilityPack {
        id: "owner_control_basic_pack".to_string(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        applies_to: AppliesTo::default(),
        candidate_allowed_actions: vec![
            ActionId::new("openspine.status.read"),
            ActionId::new("workflow.invoke:approved"),
            ActionId::new("artifact.propose"),
            ActionId::new("artifact.nominate_upstream"),
            ActionId::new("setup.workflow.start"),
            ActionId::new("memory.read:owner_preferences_limited"),
            ActionId::new("model.generate:approved_provider"),
            ActionId::new("telegram.reply:owner_channel"),
        ],
        // D-048: `artifact.activate` is the single canonical activation
        // action id (D-034 precedent) — mirrors
        // `artifacts/lyra/packs/owner_control_basic_pack.yaml`.
        approval_required: vec![
            ActionId::new("artifact.activate"),
            ActionId::new("connector.enable"),
            ActionId::new("route.activate"),
            ActionId::new("capability_pack.change"),
            ActionId::new("workflow.activate"),
            ActionId::new("policy.change_proposal"),
        ],
        denied_actions: vec![
            ActionId::new("email.read_inbox"),
            ActionId::new("email.read_thread:unselected"),
            ActionId::new("email.read_attachment"),
            ActionId::new("email.send"),
            ActionId::new("network.raw_egress"),
            ActionId::new("vault.secret_read"),
            ActionId::new("filesystem.host_read"),
            ActionId::new("filesystem.host_write"),
            ActionId::new("policy.modify_direct"),
            ActionId::new("coolify.deploy"),
            ActionId::new("coolify.rollback"),
            ActionId::new("coolify.secret_modify"),
            ActionId::new("coolify.delete_resource"),
        ],
        allowed_egress_classes: vec![],
        constraints: Constraints {
            data_classification_max: Some(DataClassification::Private),
            max_runtime_seconds: Some(120),
            ..Default::default()
        },
    }
}

pub fn selected_thread_email_draft_pack() -> CapabilityPack {
    CapabilityPack {
        id: "selected_thread_email_draft_pack".to_string(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        applies_to: AppliesTo::default(),
        candidate_allowed_actions: vec![
            ActionId::new("email.read_thread:selected_no_attachments"),
            ActionId::new("memory.read:writing_preferences_scoped"),
            ActionId::new("model.generate:approved_provider"),
            ActionId::new("artifact.write:task_scratch"),
            ActionId::new("lyra.ui.preview"),
        ],
        approval_required: vec![ActionId::new("email.create_draft")],
        denied_actions: vec![
            ActionId::new("email.send"),
            ActionId::new("email.read_inbox"),
            ActionId::new("email.read_thread:unselected"),
            ActionId::new("email.read_attachment"),
            ActionId::new("network.raw_egress"),
            ActionId::new("filesystem.host_read"),
            ActionId::new("filesystem.host_write"),
            ActionId::new("telegram.reply:owner_channel"),
        ],
        allowed_egress_classes: vec![],
        constraints: Constraints {
            data_classification_max: Some(DataClassification::Private),
            max_runtime_seconds: Some(180),
            external_communication_is_instruction: Some(false),
            ..Default::default()
        },
    }
}

pub fn global_policy() -> Policy {
    Policy {
        id: "global".to_string(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        candidate_allowed_actions: vec![],
        approval_required: vec![],
        denied_actions: vec![
            ActionId::new("email.send"),
            ActionId::new("network.raw_egress"),
            ActionId::new("vault.secret_read"),
            ActionId::new("filesystem.host_read"),
            ActionId::new("filesystem.host_write"),
            ActionId::new("policy.modify_direct"),
            ActionId::new("coolify.secret_modify"),
            ActionId::new("coolify.delete_resource"),
        ],
        constraints: Constraints {
            data_classification_max: Some(DataClassification::Private),
            ..Default::default()
        },
    }
}

pub fn empty_session_policy() -> SessionPolicy {
    SessionPolicy {
        schema_version: 1,
        candidate_allowed_actions: vec![],
        approval_required: vec![],
        denied_actions: vec![],
        constraints: Constraints::default(),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn owner_control_input<'a>(
    event: &'a EventEnvelope,
    identity: &'a IdentityResolution,
    route: &'a Route,
    agent: &'a AgentManifest,
    workflow: &'a WorkflowManifest,
    pack: &'a CapabilityPack,
    policy: &'a Policy,
    session: &'a SessionPolicy,
) -> AuthorityInput<'a> {
    AuthorityInput {
        event,
        identity,
        route,
        global_policy: policy,
        agent,
        workflow,
        pack,
        session,
        principal_id: ulid::Ulid::new(),
        purpose: "owner_control_conversation",
    }
}

/// The catalog the authority tests consult (D-053): every action id any
/// fixture builder references, so known ids compose and only a genuinely
/// unknown id trips `UnknownActionId`. Derived from the builders so it
/// can't drift from what the tests actually feed `compose_authority`.
pub fn test_catalog() -> ActionCatalog {
    let mut ids: Vec<ActionId> = Vec::new();

    let agent = main_assistant_agent();
    ids.extend(agent.designed_tools.iter().cloned());
    ids.extend(agent.approval_required_tools.iter().cloned());
    ids.extend(agent.denied_tools.iter().cloned());

    let drafter = email_reply_drafter_agent();
    ids.extend(drafter.designed_tools.iter().cloned());
    ids.extend(drafter.approval_required_tools.iter().cloned());
    ids.extend(drafter.denied_tools.iter().cloned());

    let wf = owner_control_conversation_workflow();
    ids.extend(wf.candidate_allowed_actions.iter().cloned());
    ids.extend(wf.approval_required.iter().cloned());
    ids.extend(wf.denied_actions.iter().cloned());

    let wf2 = selected_thread_email_reply_draft_workflow();
    ids.extend(wf2.candidate_allowed_actions.iter().cloned());
    ids.extend(wf2.approval_required.iter().cloned());
    ids.extend(wf2.denied_actions.iter().cloned());

    let pack = owner_control_basic_pack();
    ids.extend(pack.candidate_allowed_actions.iter().cloned());
    ids.extend(pack.approval_required.iter().cloned());
    ids.extend(pack.denied_actions.iter().cloned());

    let pack2 = selected_thread_email_draft_pack();
    ids.extend(pack2.candidate_allowed_actions.iter().cloned());
    ids.extend(pack2.approval_required.iter().cloned());
    ids.extend(pack2.denied_actions.iter().cloned());

    let policy = global_policy();
    ids.extend(policy.candidate_allowed_actions.iter().cloned());
    ids.extend(policy.approval_required.iter().cloned());
    ids.extend(policy.denied_actions.iter().cloned());

    ActionCatalog::new(ids)
}
