//! The normalized event envelope and its closed vocabularies (PRD §3, §4.1, §4.2).

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::artifact::ArtifactRef;

/// PRD §4.1 `source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Gmail,
    Telegram,
    Whatsapp,
    Slack,
    Discord,
    Cli,
    Webhook,
    Timer,
    Git,
    Internal,
}

/// PRD §4.1 `connector` (nullable in the envelope).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Connector {
    TelegramOwnerBot,
    GmailPrimaryConnector,
    GoogleWorkspacePrimary,
    OutlookPrimary,
    ImapPrimary,
    AgentmailPrimary,
    CoolifyPrimary,
}

/// PRD §3.1 / §4.1 `account_role` (nullable in the envelope).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountRole {
    OwnerMailbox,
    AgentInbox,
    SharedWorkspaceMailbox,
    CustomerIntakeInbox,
    NotificationInbox,
    OwnerControlAccount,
    SystemAccount,
}

/// PRD §4.2's closed set of implemented event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    #[serde(rename = "telegram.owner.message")]
    TelegramOwnerMessage,
    #[serde(rename = "email.thread.selected")]
    EmailThreadSelected,
    /// A kernel-owned task deadline firing (AD-090): rides the archived
    /// `workflow.timer_fired` path, then flows through the normal
    /// route -> grant -> gate pipeline as a scheduled-internal event.
    #[serde(rename = "timer.deadline.fired")]
    TimerDeadlineFired,
    /// A kernel-owned task reminder firing (AD-090): same path as a deadline.
    #[serde(rename = "timer.reminder.fired")]
    TimerReminderFired,
}

/// PRD §4.1 `verification_method`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationMethod {
    OauthSession,
    WebhookSignature,
    LocalCliAuth,
    DeviceSession,
    ConnectorPoll,
    TelegramOwnerIdMatch,
    KernelUiSelection,
    None,
}

/// PRD §4.1 `data_classification`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataClassification {
    Private,
    Internal,
    Public,
    Unknown,
}

/// PRD §3.2 lane taxonomy, plus the envelope's `internal` catch-all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    OwnerControl,
    ExternalCommunication,
    ContentDocument,
    SystemOperations,
    ScheduledInternal,
    Development,
    BusinessWorkflow,
    Internal,
}

/// PRD §4.1 `trust_context.channel_trust` (also used by identity resolution and routes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelTrust {
    OwnerDevice,
    VerifiedOwnerChannel,
    VerifiedContact,
    KnownContact,
    WorkspaceMember,
    Unknown,
    Untrusted,
}

/// PRD §4.1 `trust_context.interaction_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionMode {
    OwnerMessage,
    UserSelected,
    InboundMessage,
    Scheduled,
    SystemHook,
}

/// PRD §4.1 `trust_context`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustContext {
    pub channel_trust: ChannelTrust,
    pub interaction_mode: InteractionMode,
}

/// PRD §4.1 `actor_hint` — spoofable identifiers only; never proof of identity (PRD §4.3).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ActorHint {
    pub channel_user_id: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub handle: Option<String>,
    pub device_id: Option<String>,
}

/// PRD §4.1 `target_refs[].type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetRefKind {
    EmailThread,
    Conversation,
    Project,
    Deployment,
    SecretSlot,
    None,
}

/// PRD §4.1 `target_refs[]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetRef {
    #[serde(rename = "type")]
    pub kind: TargetRefKind,
    pub id: Option<String>,
}

/// The normalized event envelope (PRD §4.1). Every incoming channel activity
/// becomes one of these before identity resolution or routing runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventEnvelope {
    pub id: Ulid,
    pub source: Source,
    pub connector: Option<Connector>,
    pub account_role: Option<AccountRole>,
    pub event_type: EventType,
    pub received_at: jiff::Timestamp,
    pub verified_source: bool,
    pub verification_method: VerificationMethod,
    pub replay_protected: bool,
    pub replay_nonce: Option<String>,
    pub channel_account: String,
    pub raw_event_ref: ArtifactRef,
    #[serde(default)]
    pub actor_hint: ActorHint,
    #[serde(default)]
    pub target_refs: Vec<TargetRef>,
    pub data_classification: DataClassification,
    pub user_intent_hint: Option<String>,
    pub lane: Lane,
    pub trust_context: TrustContext,
    /// Dormant channel-thread binding (AD-148). None until a thread-capable
    /// channel ships; kernel-owned, never set by the shell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub schema_version: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_ref() -> ArtifactRef {
        ArtifactRef {
            digest: crate::digest::Digest::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
            schema_version: 1,
        }
    }

    fn sample_envelope() -> EventEnvelope {
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
            channel_account: "123456".to_string(),
            raw_event_ref: sample_ref(),
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

    #[test]
    fn round_trips_through_serde() {
        let e = sample_envelope();
        let json = serde_json::to_string(&e).unwrap();
        let back: EventEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn legacy_without_thread_id_defaults_to_none() {
        let mut value = serde_json::to_value(sample_envelope()).unwrap();
        value.as_object_mut().unwrap().remove("thread_id");
        let back: EventEnvelope = serde_json::from_value(value).unwrap();
        assert!(back.thread_id.is_none());
    }

    #[test]
    fn thread_id_round_trips_when_populated() {
        let mut envelope = sample_envelope();
        envelope.thread_id = Some("topic-42".to_string());
        let value = serde_json::to_value(&envelope).unwrap();
        assert_eq!(value["thread_id"], "topic-42");
        let back: EventEnvelope = serde_json::from_value(value).unwrap();
        assert_eq!(back.thread_id.as_deref(), Some("topic-42"));
    }

    #[test]
    fn event_type_serializes_as_dotted_string() {
        let json = serde_json::to_value(EventType::TelegramOwnerMessage).unwrap();
        assert_eq!(json, serde_json::json!("telegram.owner.message"));
        let json = serde_json::to_value(EventType::EmailThreadSelected).unwrap();
        assert_eq!(json, serde_json::json!("email.thread.selected"));
    }

    #[test]
    fn timer_deadline_fired_round_trips() {
        let json = serde_json::to_value(EventType::TimerDeadlineFired).unwrap();
        assert_eq!(json, serde_json::json!("timer.deadline.fired"));
        let back: EventType = serde_json::from_value(json).unwrap();
        assert_eq!(back, EventType::TimerDeadlineFired);
    }

    #[test]
    fn timer_reminder_fired_round_trips() {
        let json = serde_json::to_value(EventType::TimerReminderFired).unwrap();
        assert_eq!(json, serde_json::json!("timer.reminder.fired"));
        let back: EventType = serde_json::from_value(json).unwrap();
        assert_eq!(back, EventType::TimerReminderFired);
    }

    #[test]
    fn unknown_field_is_rejected() {
        let mut value = serde_json::to_value(sample_envelope()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("capability_pack_id".into(), serde_json::json!("sneaky"));
        let err = serde_json::from_value::<EventEnvelope>(value).unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn actor_hint_defaults_to_all_none() {
        let hint = ActorHint::default();
        assert!(hint.channel_user_id.is_none() && hint.email.is_none() && hint.phone.is_none());
    }
}
