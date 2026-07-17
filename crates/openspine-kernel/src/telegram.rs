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
use teloxide::types::{CallbackQueryId, InlineKeyboardButton, InlineKeyboardMarkup};
use ulid::Ulid;

/// Minimal, testable projection of one Telegram update.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
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
    /// Set only for a tap on an inline keyboard button (D-039) — `text`
    /// is always `None` on the same update, never both at once.
    pub callback_query: Option<CallbackQueryUpdate>,
}

/// A tap on an inline keyboard button. `id` must be echoed back via
/// `answerCallbackQuery` (stops the tapping client's loading spinner);
/// `data` is the button's `callback_data`, `None` only for Telegram's own
/// game/`inline_message_id` callback shapes this connector never sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallbackQueryUpdate {
    pub id: String,
    pub data: Option<String>,
}

fn project_update(update: &teloxide::types::Update) -> TelegramUpdate {
    let (chat_id, is_private_chat, sender_user_id, text, callback_query) = match &update.kind {
        teloxide::types::UpdateKind::Message(msg) => (
            Some(msg.chat.id.0),
            msg.chat.is_private(),
            msg.from.as_ref().map(|u| u.id.0 as i64),
            msg.text().map(str::to_string),
            None,
        ),
        teloxide::types::UpdateKind::CallbackQuery(cb) => {
            let chat = cb.message.as_ref().map(|m| m.chat());
            (
                chat.map(|c| c.id.0),
                chat.is_some_and(|c| c.is_private()),
                Some(cb.from.id.0 as i64),
                None,
                Some(CallbackQueryUpdate {
                    id: cb.id.0.clone(),
                    data: cb.data.clone(),
                }),
            )
        }
        _ => (None, false, None, None, None),
    };
    TelegramUpdate {
        update_id: update.id.0 as i64,
        chat_id: chat_id.unwrap_or(0),
        is_private_chat,
        sender_user_id,
        text,
        callback_query,
    }
}

/// A token proving owner verification. Can only be constructed inside `telegram.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedOwnerContext {
    _private: (),
}

#[cfg(test)]
impl VerifiedOwnerContext {
    pub(crate) fn test_new() -> Self {
        Self { _private: () }
    }
}

/// Outcome of verifying one update against the configured owner id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifiedUpdate {
    /// A text message from the configured owner, in their private chat.
    OwnerMessage {
        chat_id: i64,
        text: String,
        context: VerifiedOwnerContext,
    },
    /// A tap on an inline keyboard button from the configured owner, in
    /// their private chat (D-039) — same verification guarantee as
    /// `OwnerMessage`, just a different input shape.
    OwnerCallback {
        chat_id: i64,
        callback_query_id: String,
        data: String,
        context: VerifiedOwnerContext,
    },
    /// Anything else — non-owner sender, no sender, a non-text/callback
    /// update, or (even from the owner) a non-private chat. Audited and
    /// ignored, never routed (spec.md: "the event MUST NOT receive
    /// owner-control authority").
    Ignored { reason: &'static str },
}

/// Verify one update against `owner_user_id`. Pure function — the entire
/// owner-verification decision lives here, unit-tested exhaustively.
/// Requires BOTH the sender id match AND a private chat: a message (or
/// callback tap) from the owner in a group they belong to must never be
/// treated as owner-control (the reply/effect would be visible to the
/// whole group).
pub fn verify_update(update: &TelegramUpdate, owner_user_id: i64) -> VerifiedUpdate {
    if let Some(cb) = &update.callback_query {
        return match (update.sender_user_id, &cb.data) {
            (Some(uid), Some(_)) if uid == owner_user_id && !update.is_private_chat => {
                VerifiedUpdate::Ignored {
                    reason: "owner_message_outside_private_chat",
                }
            }
            (Some(uid), Some(data)) if uid == owner_user_id => VerifiedUpdate::OwnerCallback {
                chat_id: update.chat_id,
                callback_query_id: cb.id.clone(),
                data: data.clone(),
                context: VerifiedOwnerContext { _private: () },
            },
            (Some(_), Some(_)) => VerifiedUpdate::Ignored {
                reason: "unknown_telegram_user",
            },
            (None, _) => VerifiedUpdate::Ignored {
                reason: "no_sender",
            },
            (_, None) => VerifiedUpdate::Ignored {
                reason: "callback_query_missing_data",
            },
        };
    }
    match (update.sender_user_id, &update.text) {
        (Some(uid), Some(_)) if uid == owner_user_id && !update.is_private_chat => {
            VerifiedUpdate::Ignored {
                reason: "owner_message_outside_private_chat",
            }
        }
        (Some(uid), Some(text)) if uid == owner_user_id => VerifiedUpdate::OwnerMessage {
            chat_id: update.chat_id,
            text: text.clone(),
            context: VerifiedOwnerContext { _private: () },
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

/// D-039: parse the inline "Approve" button's `callback_data`. Returns
/// `None` for anything that isn't this exact, well-formed shape (missing
/// prefix, or a suffix that isn't a valid [`Ulid`]) — a malformed or
/// foreign `callback_data` value must never be misread as approving some
/// other request.
const APPROVE_CALLBACK_PREFIX: &str = "approve_draft:";

pub fn parse_approve_callback(data: &str) -> Option<Ulid> {
    data.strip_prefix(APPROVE_CALLBACK_PREFIX)?.parse().ok()
}

/// Plan approval callback; kept distinct from draft approval so an approved
/// plan can never fall through to draft dispatch.
const APPROVE_PLAN_CALLBACK_PREFIX: &str = "approve_plan:";

pub fn parse_approve_plan_callback(data: &str) -> Option<Ulid> {
    data.strip_prefix(APPROVE_PLAN_CALLBACK_PREFIX)?
        .parse()
        .ok()
}

/// PRD §21.1 step 1 / D-036: the Phase-2 thread-selection trigger is this
/// exact structured command, recognized by the kernel itself before any
/// shell/agent ever sees the message — the shell can never claim on the
/// owner's behalf that a thread was selected. Returns `None` for anything
/// that is not `/draft <id>` with a well-formed id, so a malformed command
/// falls through to ordinary owner-control routing instead of silently
/// misparsing.
///
/// The id is validated against Gmail's thread-id alphabet
/// (`[A-Za-z0-9_-]`, in practice lowercase hex) and a generous length
/// bound — this is the *entire* trust boundary for "did the owner select
/// this thread" on the Telegram side (D-036), so a stray `/`, `?`, `&`, or
/// `#` in the text must never reach the Gmail connector's request URL.
const MAX_THREAD_ID_LEN: usize = 64;

pub fn parse_draft_command(text: &str) -> Option<&str> {
    let rest = text.trim().strip_prefix("/draft")?;
    // Require a whitespace boundary right after the literal `/draft` token
    // — without this, `/draftabc` or `/drafts` would wrongly strip to a
    // "valid-looking" id, which is exactly the fuzzy matching D-036 rules
    // out ("exact-prefix match, no fuzzy matching").
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let id = rest.trim();
    let valid = !id.is_empty()
        && id.len() <= MAX_THREAD_ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    valid.then_some(id)
}

pub fn parse_bind_command(text: &str) -> Option<(&str, &str)> {
    let rest = text.trim().strip_prefix("/bind")?;
    // Require a whitespace boundary right after the literal `/bind` token
    let rest = rest.strip_prefix(' ')?;
    let mut parts = rest.split_whitespace();
    let channel_user_id = parts.next()?;
    let relationship = parts.next()?;
    if parts.next().is_some() {
        return None; // too many arguments
    }
    Some((channel_user_id, relationship))
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
        thread_id: None,
        schema_version: 1,
    }
}

pub const BOT_TOKEN_SLOT: &str = "telegram.bot_token";

/// The live Telegram connector: long-polling plus the reply dispatcher.
pub struct TelegramConnector {
    bot: parking_lot::Mutex<TelegramBotState>,
    secrets: Option<std::sync::Arc<crate::secret_store::SecretStore>>,
    token_slot: String,
}

struct TelegramBotState {
    bot: Bot,
    token: String,
    api_url: Option<reqwest::Url>,
}

impl TelegramConnector {
    pub fn new(bot_token: String) -> Self {
        Self {
            bot: parking_lot::Mutex::new(TelegramBotState {
                bot: Bot::new(bot_token.clone()),
                token: bot_token,
                api_url: None,
            }),
            secrets: None,
            token_slot: String::new(),
        }
    }

    pub fn new_with_store(
        bot_token: String,
        secrets: std::sync::Arc<crate::secret_store::SecretStore>,
        token_slot: String,
    ) -> Self {
        let mut connector = Self::new(bot_token);
        connector.secrets = Some(secrets);
        connector.token_slot = token_slot;
        connector
    }

    async fn current_bot(&self) -> anyhow::Result<Bot> {
        let Some(secrets) = &self.secrets else {
            return Ok(self.bot.lock().bot.clone());
        };
        let token = secrets
            .get_string(&self.token_slot)
            .map_err(|err| anyhow::anyhow!("telegram bot token lookup failed: {err}"))?
            .ok_or_else(|| anyhow::anyhow!("telegram bot token is not configured"))?;
        let mut state = self.bot.lock();
        if state.token != token {
            let mut bot = Bot::new(token.clone());
            if let Some(api_url) = &state.api_url {
                bot = bot.set_api_url(api_url.clone());
            }
            state.bot = bot;
            state.token = token;
        }
        Ok(state.bot.clone())
    }

    pub async fn validate_candidate_token_id(&self, candidate: &str) -> Option<i64> {
        let api_url = self.bot.lock().api_url.clone();
        let mut bot = Bot::new(candidate.to_string());
        if let Some(url) = api_url {
            bot = bot.set_api_url(url);
        }
        bot.get_me().send().await.ok().map(|user| user.id.0 as i64)
    }

    #[cfg(test)]
    pub(crate) async fn current_token_for_test(&self) -> anyhow::Result<String> {
        let _ = self.current_bot().await?;
        Ok(self.bot.lock().token.clone())
    }

    #[cfg(test)]
    pub fn with_api_url(bot_token: String, api_url: reqwest::Url) -> Self {
        let connector = Self::new(bot_token);
        let mut state = connector.bot.lock();
        state.api_url = Some(api_url.clone());
        state.bot = state.bot.clone().set_api_url(api_url);
        drop(state);
        connector
    }
    /// Test-only: a vault-backed connector (`new_with_store`) whose bot API
    /// URL is pointed at a mock server — so after a token rotation the live
    /// `current_bot()` reads the promoted token from the vault and polls
    /// with it, exercising the production promotion path end to end.
    #[cfg(test)]
    pub(crate) fn with_store_and_api_url(
        bot_token: String,
        secrets: std::sync::Arc<crate::secret_store::SecretStore>,
        token_slot: String,
        api_url: reqwest::Url,
    ) -> Self {
        let connector = Self::new_with_store(bot_token, secrets, token_slot);
        let mut state = connector.bot.lock();
        state.api_url = Some(api_url.clone());
        state.bot = state.bot.clone().set_api_url(api_url);
        drop(state);
        connector
    }
    /// `offset` (the last processed `update_id`, or `None` for "everything
    /// currently queued"). Telegram's own `offset` semantics are "greater
    /// by one than the highest previously received id", so this adds 1
    /// when an offset is given.
    pub async fn poll_once(
        &self,
        last_update_id: Option<i64>,
    ) -> anyhow::Result<Vec<TelegramUpdate>> {
        let bot = self.current_bot().await?;
        let mut request = bot.get_updates();
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
    pub async fn send_reply(&self, chat_id: i64, text: &str) -> anyhow::Result<()> {
        self.current_bot()
            .await?
            .send_message(ChatId(chat_id), text)
            .await?;
        Ok(())
    }

    /// Send `text` to `chat_id` with a single inline "Approve" button
    /// (D-039/D-043) whose `callback_data` names `action_request_id` —
    /// [`parse_approve_callback`] is the only thing that ever reads it
    /// back. Same channel-binding caveat as [`Self::send_reply`]: the
    /// caller must already have verified `chat_id`.
    pub async fn send_reply_with_approval_button(
        &self,
        chat_id: i64,
        text: &str,
        action_request_id: Ulid,
    ) -> anyhow::Result<()> {
        let button = InlineKeyboardButton::callback(
            "Approve",
            format!("{APPROVE_CALLBACK_PREFIX}{action_request_id}"),
        );
        let markup = InlineKeyboardMarkup::default().append_row(vec![button]);
        self.current_bot()
            .await?
            .send_message(ChatId(chat_id), text)
            .reply_markup(markup)
            .await?;
        Ok(())
    }

    /// Send a complete plan question with a plan-specific approval callback.
    pub async fn send_reply_with_plan_approval_button(
        &self,
        chat_id: i64,
        text: &str,
        action_request_id: Ulid,
    ) -> anyhow::Result<()> {
        let button = InlineKeyboardButton::callback(
            "Approve plan",
            format!("{APPROVE_PLAN_CALLBACK_PREFIX}{action_request_id}"),
        );
        let markup = InlineKeyboardMarkup::default().append_row(vec![button]);
        self.current_bot()
            .await?
            .send_message(ChatId(chat_id), text)
            .reply_markup(markup)
            .await?;
        Ok(())
    }

    /// Stop the tapping client's loading spinner (D-039). Best-effort:
    /// the approval decision itself is already recorded by the time this
    /// is called, so a failure here is logged, never propagated — the
    /// owner's tap has already done its job regardless of whether
    /// Telegram's own UI acknowledgment succeeds.
    pub async fn answer_callback_query(&self, callback_query_id: &str) {
        let Ok(bot) = self.current_bot().await else {
            return;
        };
        if let Err(err) = bot
            .answer_callback_query(CallbackQueryId(callback_query_id.to_string()))
            .await
        {
            tracing::warn!(error = %err, "failed to answer Telegram callback query");
        }
    }
}

#[cfg(test)]
mod tests;
