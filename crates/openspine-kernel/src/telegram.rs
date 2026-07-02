//! Telegram connector (build plan 4b): long-polling, owner-id verification,
//! and the reply dispatcher.
//!
//! Owner verification is the single most safety-critical piece of Phase 1
//! (design.md: "Only the configured Telegram owner user ID qualifies for
//! owner-control routing"). The verification and envelope-building logic is
//! kept as pure functions over a small, hand-rolled [`TelegramUpdate`]
//! projection — not `teloxide::types::Update` directly — so it is fully
//! unit-testable without constructing teloxide's full wire types or
//! standing up a live bot.

use jiff::Timestamp;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::event::{
    AccountRole, ActorHint, ChannelTrust, Connector, DataClassification, EventEnvelope, EventType,
    InteractionMode, Lane, Source, TrustContext, VerificationMethod,
};
use teloxide::prelude::*;
use ulid::Ulid;

/// Minimal, testable projection of one Telegram update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramUpdate {
    pub update_id: i64,
    pub chat_id: i64,
    /// Telegram private (1:1) chats have `chat.id == sender.user_id`; a
    /// group/supergroup/channel is not private even if the owner is a
    /// member and happens to send the message. Owner-control routing
    /// requires both "the owner sent it" AND "no one else can see the
    /// reply" (design.md: "a guarded workflow... no one else can reach you
    /// here") — matching only `sender_user_id` would let the owner's
    /// replies leak to every other member of a group they're in.
    pub is_private_chat: bool,
    pub sender_user_id: Option<i64>,
    pub text: Option<String>,
}

fn project_update(update: &teloxide::types::Update) -> TelegramUpdate {
    let (chat_id, is_private_chat, sender_user_id, text) = match &update.kind {
        teloxide::types::UpdateKind::Message(msg) => (
            Some(msg.chat.id.0),
            msg.chat.is_private(),
            msg.from.as_ref().map(|u| u.id.0 as i64),
            msg.text().map(str::to_string),
        ),
        _ => (None, false, None, None),
    };
    TelegramUpdate {
        update_id: update.id.0 as i64,
        chat_id: chat_id.unwrap_or(0),
        is_private_chat,
        sender_user_id,
        text,
    }
}

/// Outcome of verifying one update against the configured owner id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifiedUpdate {
    /// A text message from the configured owner, in their private chat.
    OwnerMessage { chat_id: i64, text: String },
    /// Anything else — non-owner sender, no sender, a non-text update, or
    /// (even from the owner) a non-private chat. Audited and ignored,
    /// never routed (spec.md: "the event MUST NOT receive owner-control
    /// authority").
    Ignored { reason: &'static str },
}

/// Verify one update against `owner_user_id`. Pure function — the entire
/// owner-verification decision lives here, unit-tested exhaustively.
/// Requires BOTH the sender id match AND a private chat: a message from
/// the owner posted in a group they belong to must never be treated as
/// owner-control (the reply would be visible to the whole group).
pub fn verify_update(update: &TelegramUpdate, owner_user_id: i64) -> VerifiedUpdate {
    match (update.sender_user_id, &update.text) {
        (Some(uid), Some(_)) if uid == owner_user_id && !update.is_private_chat => {
            VerifiedUpdate::Ignored {
                reason: "owner_message_outside_private_chat",
            }
        }
        (Some(uid), Some(text)) if uid == owner_user_id => VerifiedUpdate::OwnerMessage {
            chat_id: update.chat_id,
            text: text.clone(),
        },
        (Some(_), Some(_)) => VerifiedUpdate::Ignored {
            reason: "unknown_telegram_user",
        },
        (None, _) => VerifiedUpdate::Ignored {
            reason: "no_sender",
        },
        (_, None) => VerifiedUpdate::Ignored {
            reason: "non_text_update",
        },
    }
}

/// Build the PRD §4.1/§4.2A `telegram.owner.message` envelope for a
/// verified owner message. `raw_event_ref` must already point at the raw
/// message text encrypted in the artifact store — this function never
/// touches plaintext.
pub fn build_owner_envelope(
    chat_id: i64,
    raw_event_ref: ArtifactRef,
    now: Timestamp,
) -> EventEnvelope {
    EventEnvelope {
        id: Ulid::new(),
        source: Source::Telegram,
        connector: Some(Connector::TelegramOwnerBot),
        account_role: Some(AccountRole::OwnerControlAccount),
        event_type: EventType::TelegramOwnerMessage,
        received_at: now,
        verified_source: true,
        verification_method: VerificationMethod::TelegramOwnerIdMatch,
        replay_protected: true,
        replay_nonce: None,
        channel_account: chat_id.to_string(),
        raw_event_ref,
        actor_hint: ActorHint {
            channel_user_id: Some(chat_id.to_string()),
            ..Default::default()
        },
        target_refs: vec![],
        data_classification: DataClassification::Private,
        user_intent_hint: None,
        lane: Lane::OwnerControl,
        trust_context: TrustContext {
            channel_trust: ChannelTrust::VerifiedOwnerChannel,
            interaction_mode: InteractionMode::OwnerMessage,
        },
        schema_version: 1,
    }
}

/// The live Telegram connector: long-polling plus the reply dispatcher.
/// Thin wrapper over [`teloxide::Bot`] — every decision this connector
/// makes lives in [`verify_update`]/[`build_owner_envelope`] above, not
/// here, so the untested I/O surface is as small as possible.
pub struct TelegramConnector {
    bot: Bot,
}

impl TelegramConnector {
    pub fn new(bot_token: String) -> Self {
        Self {
            bot: Bot::new(bot_token),
        }
    }

    /// Build a connector that redirects all Bot API calls to `api_url`.
    /// Used by tests to point [`send_reply`] and [`poll_once`] at a local
    /// `wiremock` server instead of the real `api.telegram.org`.
    #[cfg(test)]
    pub fn with_api_url(bot_token: String, api_url: reqwest::Url) -> Self {
        Self {
            bot: Bot::new(bot_token).set_api_url(api_url),
        }
    }

    /// Fetch one batch of updates via long-polling, starting after
    /// `offset` (the last processed `update_id`, or `None` for "everything
    /// currently queued"). Telegram's own `offset` semantics are "greater
    /// by one than the highest previously received id", so this adds 1
    /// when an offset is given.
    pub async fn poll_once(
        &self,
        last_update_id: Option<i64>,
    ) -> anyhow::Result<Vec<TelegramUpdate>> {
        let mut request = self.bot.get_updates();
        if let Some(id) = last_update_id {
            request = request.offset((id + 1) as i32);
        }
        request = request.timeout(30);
        let updates = request.send().await?;
        Ok(updates.iter().map(project_update).collect())
    }

    /// Send a reply to `chat_id`. The caller (the kernel's reply
    /// dispatcher) is responsible for verifying `chat_id` matches the
    /// grant-bound owner chat before ever calling this — this function
    /// itself performs no channel-binding check (spec.md's
    /// `Deny(ChannelBindingViolation)` requirement lives in the dispatcher,
    /// gate()-mediated, not here).
    pub async fn send_reply(&self, chat_id: i64, text: &str) -> anyhow::Result<()> {
        self.bot.send_message(ChatId(chat_id), text).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(sender: Option<i64>, text: Option<&str>) -> TelegramUpdate {
        TelegramUpdate {
            update_id: 1,
            chat_id: 555,
            is_private_chat: true,
            sender_user_id: sender,
            text: text.map(str::to_string),
        }
    }

    #[test]
    fn configured_owner_text_message_is_verified() {
        let result = verify_update(&update(Some(42), Some("hello")), 42);
        assert_eq!(
            result,
            VerifiedUpdate::OwnerMessage {
                chat_id: 555,
                text: "hello".to_string()
            }
        );
    }

    #[test]
    fn unknown_telegram_user_is_ignored_not_routed() {
        let result = verify_update(&update(Some(99), Some("hello")), 42);
        assert_eq!(
            result,
            VerifiedUpdate::Ignored {
                reason: "unknown_telegram_user"
            }
        );
    }

    #[test]
    fn missing_sender_is_ignored() {
        let result = verify_update(&update(None, Some("hello")), 42);
        assert_eq!(
            result,
            VerifiedUpdate::Ignored {
                reason: "no_sender"
            }
        );
    }

    #[test]
    fn non_text_update_from_owner_is_ignored() {
        let result = verify_update(&update(Some(42), None), 42);
        assert_eq!(
            result,
            VerifiedUpdate::Ignored {
                reason: "non_text_update"
            }
        );
    }

    #[test]
    fn owner_message_in_a_group_chat_is_ignored_not_routed() {
        // The owner is a member of some group and sends a message there —
        // sender id matches, but the chat is not private, so this must
        // never become owner-control routing (the reply would be visible
        // to every other group member).
        let mut group_update = update(Some(42), Some("hello"));
        group_update.is_private_chat = false;
        let result = verify_update(&group_update, 42);
        assert_eq!(
            result,
            VerifiedUpdate::Ignored {
                reason: "owner_message_outside_private_chat"
            }
        );
    }

    #[test]
    fn owner_envelope_is_verified_with_owner_id_match_method() {
        let raw_ref = ArtifactRef {
            digest: openspine_schemas::digest::Digest::parse(format!("sha256:{}", "a".repeat(64)))
                .unwrap(),
            schema_version: 1,
        };
        let envelope = build_owner_envelope(555, raw_ref, Timestamp::now());
        assert!(envelope.verified_source);
        assert_eq!(
            envelope.verification_method,
            VerificationMethod::TelegramOwnerIdMatch
        );
        assert_eq!(envelope.event_type, EventType::TelegramOwnerMessage);
        assert_eq!(envelope.lane, Lane::OwnerControl);
        assert_eq!(envelope.channel_account, "555");
    }
}
