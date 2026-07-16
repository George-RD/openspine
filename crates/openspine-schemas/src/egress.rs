//! Egress endpoint classes (AD-060).
//!
//! Connectors expose typed egress endpoints — a no-log search API is not a
//! forum browse is not a web-form POST. The connector registry rates each
//! endpoint with one of these classes; capability packs reference the
//! classes they permit, and the gate denies an action whose rated class is
//! not covered by the grant.
//!
//! This is a closed enum for phases 0–3: the three classes AD-060 names.
//! Adding a class is a deliberate, reviewed change (new variant + catalog
//! + registry rating), not a dynamic string.

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EgressClass {
    Search,
    ForumBrowse,
    WebFormPost,
}

impl EgressClass {
    /// Stable kebab-case identifier used in canonical JSON / audit logs.
    pub fn as_str(&self) -> &'static str {
        match self {
            EgressClass::Search => "search",
            EgressClass::ForumBrowse => "forum-browse",
            EgressClass::WebFormPost => "web-form-post",
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
    }
}
