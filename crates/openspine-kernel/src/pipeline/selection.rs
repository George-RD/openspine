//! Selection-flow helpers for the email-preview pipeline lane.
//!
//! This module no longer contains a driver function. The single-use
//! [`SelectionToken`] minted to prove the owner actually selected a Gmail
//! thread (PRD §15: "expires quickly") is built here and bound by the
//! email-preview lane's grant-binding hook in `driver.rs`. The derived
//! pending-message prompt the lane persists as the task's pending input
//! ([`format_pending_message`]) is also defined here.

use jiff::Timestamp;
use openspine_schemas::event::{AccountRole, Connector};
use openspine_schemas::selection::{
    SelectionScope, SelectionToken, SelectionTokenType, SelectionVerificationMethod,
};
use ulid::Ulid;

use super::AppState;

const SELECTION_TOKEN_TTL: std::time::Duration = std::time::Duration::from_secs(300);

/// Build the single-use selection token that proves the owner selected this
/// Gmail thread. Shared by the email-preview lane's grant-binding hook.
pub(super) fn build_selection_token(
    state: &AppState,
    thread_id: &str,
    now: Timestamp,
) -> SelectionToken {
    let user = state.owner_user_id.to_string();
    SelectionToken {
        id: Ulid::new(),
        schema_version: 1,
        token_type: SelectionTokenType::email_thread_selection(),
        user: user.clone(),
        target_id: thread_id.to_string(),
        selected_by: user.clone(),
        selected_at: now,
        issued_by: "kernel".to_string(),
        expires_at: now + SELECTION_TOKEN_TTL,
        verified_source: true,
        verification_method: SelectionVerificationMethod::ApprovedOwnerControlSelection,
        connector: Some(Connector::GmailPrimaryConnector),
        account_role: Some(AccountRole::OwnerMailbox),
        scope: SelectionScope {
            read_thread: true,
            attachments_allowed: false,
            max_messages: 20,
            include_headers: true,
            include_recipients: true,
            include_body: true,
        },
        single_use: true,
    }
}

/// The derived pending-message prompt the email-preview lane persists as the
/// task's pending input (PRD §21.1) — deliberately distinct from the raw
/// `/draft <id>` command the owner typed.
pub(super) fn format_pending_message(thread_id: &str, token_id: Ulid) -> String {
    format!("Draft a reply to Gmail thread {thread_id} (selection token {token_id})")
}
