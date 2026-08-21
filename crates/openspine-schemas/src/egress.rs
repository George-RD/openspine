//! Egress endpoint classes (AD-060).
//!
//! Connectors expose typed egress endpoints — a no-log search API is not a
//! forum browse is not a web-form POST. The connector registry rates each
//! endpoint with one of these classes; capability packs reference the
//! classes they permit, and the gate denies an action whose rated class is
//! not covered by the grant.
//!
//! This enum grows only by deliberate, reviewed change (a new variant plus its
//! catalog and registry rating), never a dynamic string. It started as the
//! three web classes AD-060 names (`Search`/`ForumBrowse`/`WebFormPost`) and
//! gained `DirectMessage` for messaging-style external sends (spec #204).

use serde::{Deserialize, Serialize};

/// The policy-rated class of one egress endpoint (AD-060).
///
/// `Search` — read-only query against an external search/index API
/// (generalized queries, no side effects).
/// `ForumBrowse` — read-only browse/fetch of public or accessible forum
/// or feed content.
/// `WebFormPost` — submitting data to an external web form or API endpoint
/// that accepts user-supplied content (side-effecting, potentially
/// irreversible).
/// `DirectMessage` — a messaging-style send of composed kernel content to one
/// verified recipient over a bound channel (email, Telegram, future WhatsApp);
/// they share the same risk shape — content addressed to a single recipient
/// via a selection token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EgressClass {
    Search,
    ForumBrowse,
    WebFormPost,
    DirectMessage,
}

impl EgressClass {
    /// Stable kebab-case identifier used in canonical JSON / audit logs.
    pub fn as_str(&self) -> &'static str {
        match self {
            EgressClass::Search => "search",
            EgressClass::ForumBrowse => "forum-browse",
            EgressClass::WebFormPost => "web-form-post",
            EgressClass::DirectMessage => "direct-message",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_serde() {
        for class in [
            EgressClass::Search,
            EgressClass::ForumBrowse,
            EgressClass::WebFormPost,
            EgressClass::DirectMessage,
        ] {
            let json = serde_json::to_string(&class).unwrap();
            let back: EgressClass = serde_json::from_str(&json).unwrap();
            assert_eq!(class, back);
        }
    }

    #[test]
    fn serde_uses_kebab_case() {
        assert_eq!(
            serde_json::to_string(&EgressClass::ForumBrowse).unwrap(),
            "\"forum-browse\""
        );
        assert_eq!(
            serde_json::to_string(&EgressClass::WebFormPost).unwrap(),
            "\"web-form-post\""
        );
        assert_eq!(
            serde_json::to_string(&EgressClass::DirectMessage).unwrap(),
            "\"direct-message\""
        );
    }
}
