//! Selection tokens (PRD §15). The shell must not be trusted to provide
//! target IDs and claim the user selected them — the kernel mints these,
//! the shell can only present the id it was given.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::event::{AccountRole, Connector};

/// PRD §15 `type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionTokenType {
    EmailThreadSelection,
}

/// PRD §15 `verification_method`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionVerificationMethod {
    KernelUiSelection,
    ApprovedOwnerControlSelection,
}

/// PRD §15 `scope`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionScope {
    pub read_thread: bool,
    pub attachments_allowed: bool,
    pub max_messages: u32,
    pub include_headers: bool,
    pub include_recipients: bool,
    pub include_body: bool,
}

fn default_true() -> bool {
    true
}

/// A selection token (PRD §15).
///
/// `single_use` is not in the PRD's literal example; Step 5 of the build
/// plan requires selection tokens to be single-use (marked used on first
/// successful gate dispatch), so it is recorded here explicitly rather than
/// left implicit. Defaults to `true` — the PRD's rules ("expires quickly",
/// "only usable inside matching task grant") describe a one-shot token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionToken {
    pub id: Ulid,
    pub schema_version: u32,
    #[serde(rename = "type")]
    pub token_type: SelectionTokenType,
    pub user: String,
    pub target_id: String,
    pub selected_by: String,
    pub selected_at: jiff::Timestamp,
    pub issued_by: String,
    pub expires_at: jiff::Timestamp,
    pub verified_source: bool,
    pub verification_method: SelectionVerificationMethod,
    pub connector: Option<Connector>,
    pub account_role: Option<AccountRole>,
    pub scope: SelectionScope,
    #[serde(default = "default_true")]
    pub single_use: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::Timestamp;

    fn sample_token() -> SelectionToken {
        let now = Timestamp::now();
        SelectionToken {
            id: Ulid::new(),
            schema_version: 1,
            token_type: SelectionTokenType::EmailThreadSelection,
            user: "owner".to_string(),
            target_id: "thread_abc123".to_string(),
            selected_by: "owner".to_string(),
            selected_at: now,
            issued_by: "kernel".to_string(),
            expires_at: now + std::time::Duration::from_secs(600),
            verified_source: true,
            verification_method: SelectionVerificationMethod::KernelUiSelection,
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

    #[test]
    fn round_trips_through_serde() {
        let token = sample_token();
        let json = serde_json::to_string(&token).unwrap();
        let back: SelectionToken = serde_json::from_str(&json).unwrap();
        assert_eq!(token, back);
    }

    #[test]
    fn single_use_defaults_to_true_when_omitted() {
        let mut json = serde_json::to_value(sample_token()).unwrap();
        json.as_object_mut().unwrap().remove("single_use");
        let token: SelectionToken = serde_json::from_value(json).unwrap();
        assert!(token.single_use);
    }

    #[test]
    fn attachments_are_never_allowed_in_the_email_scope() {
        assert!(!sample_token().scope.attachments_allowed);
    }
}
