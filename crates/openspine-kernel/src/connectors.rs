//! Connector registry (kernel registry refactor, part 2).
//!
//! Connectors are held in a single registry that is the one registration
//! point for connector instances. Gmail's optionality is preserved
//! bit-for-bit: the registry reports it absent when unconfigured so call
//! sites keep their graceful-degradation branches (see
//! `pipeline::driver::email_preview_lane` and
//! `pipeline::approval::create_approved_draft`).

use crate::gmail::GmailConnector;
use crate::telegram::TelegramConnector;

/// A kernel connector. The trait is the future AD-060 / AD-103 registration
/// seam; today it only names the connector so the registry can enumerate
/// configured connectors uniformly.
pub trait Connector {
    #[allow(dead_code)] // the AD-060/AD-103 enumeration seam; exercised via `iter()` in tests today
    fn name(&self) -> &'static str;
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

/// The kernel's connector registry: the single registration point for
/// connector instances. `telegram` is always present (the long-poll loop
/// depends on it); `gmail` is optional and reported as absent when
/// unconfigured.
pub struct ConnectorRegistry {
    telegram: TelegramConnector,
    gmail: Option<GmailConnector>,
}

impl ConnectorRegistry {
    pub fn new(telegram: TelegramConnector, gmail: Option<GmailConnector>) -> Self {
        Self { telegram, gmail }
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

#[cfg(test)]
mod tests {
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
        let absent = ConnectorRegistry::new(TelegramConnector::new("t".to_string()), None);
        let names: Vec<&str> = absent.iter().map(Connector::name).collect();
        assert_eq!(names, vec!["telegram"]);

        // Gmail present.
        let present =
            ConnectorRegistry::new(TelegramConnector::new("t".to_string()), Some(gmail()));
        let names: Vec<&str> = present.iter().map(Connector::name).collect();
        assert_eq!(names, vec!["telegram", "gmail"]);

        // Accessors reflect configuration.
        assert!(present.gmail().is_some());
        assert!(absent.gmail().is_none());
        assert_eq!(present.telegram().name(), "telegram");
    }
}
