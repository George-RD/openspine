//! Connector registry (kernel registry refactor, part 2 + AD-060).
//!
//! Connectors are held in a single registry that is the one registration
//! point for connector instances. Gmail's optionality is preserved
//! bit-for-bit: the registry reports it absent when unconfigured so call
//! sites keep their graceful-degradation branches (see
//! `pipeline::driver::email_preview_lane` and
//! `pipeline::approval::create_approved_draft`).
//!
//! AD-060: the registry is also the source of truth for egress-endpoint
//! ratings. Endpoint → class mappings live here — not on the request —
//! and the gate queries them through [`EgressClassifier`].

use std::collections::{hash_map::Entry, HashMap};

use crate::connector_reality::{
    BreakerState, CircuitBreakerConfig, ConnectorProbePermit, ConnectorRuntime, RateLimitConfig,
};
use crate::gmail::GmailConnector;
use crate::telegram::TelegramConnector;
use jiff::Timestamp;
use openspine_gate::EgressClassifier;
use openspine_schemas::action::ActionId;
use openspine_schemas::egress::EgressClass;
use parking_lot::Mutex;

/// A kernel connector. The trait is the AD-060 / AD-103 registration
/// seam: connectors declare their name and any rated egress endpoints.
pub trait Connector {
    #[allow(dead_code)] // the AD-060/AD-103 enumeration seam; exercised via `iter()` in tests today
    fn name(&self) -> &'static str;

    /// AD-060: egress endpoints this connector exposes, each rated with
    /// its egress class. Connectors with no rated egress return empty.
    fn egress_endpoints(&self) -> Vec<(ActionId, EgressClass)> {
        Vec::new()
    }
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

/// Built-in AD-060 web-egress endpoints rated in the connector registry.
/// These represent the web egress surface packs authorize by class; a
/// future web connector will dispatch them, but the rating lives here so
/// the gate can enforce class coverage before any dispatcher exists.
fn built_in_web_egress_endpoints() -> Vec<(ActionId, EgressClass)> {
    vec![
        (ActionId::new("web.search"), EgressClass::Search),
        (ActionId::new("web.forum_browse"), EgressClass::ForumBrowse),
        (ActionId::new("web.form_submit"), EgressClass::WebFormPost),
    ]
}

/// A conflicting endpoint rating is rejected rather than silently changing
/// the policy class of an already-registered endpoint (AD-060).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressRegistrationError {
    pub action: ActionId,
    pub existing: EgressClass,
    pub requested: EgressClass,
}

impl std::fmt::Display for EgressRegistrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "egress endpoint {} already rated {:?}, cannot register {:?}",
            self.action, self.existing, self.requested
        )
    }
}

impl std::error::Error for EgressRegistrationError {}

/// The kernel's connector registry: the single registration point for
/// connector instances and for egress-endpoint class ratings (AD-060).
/// `telegram` is always present (the long-poll loop depends on it);
/// `gmail` is optional and reported as absent when unconfigured.
pub struct ConnectorRegistry {
    telegram: TelegramConnector,
    gmail: Option<GmailConnector>,
    /// Aggregated endpoint → class ratings from built-in web egress plus
    /// every configured connector's `egress_endpoints()`.
    egress_ratings: HashMap<ActionId, EgressClass>,
    /// Per-connector admission control (rate limit + circuit breaker),
    /// keyed by connector name. Mutated behind a lock so shared `AppState`
    /// references can record outcomes from any dispatch path.
    runtimes: Mutex<HashMap<String, ConnectorRuntime>>,
}
impl ConnectorRegistry {
    pub fn new(
        telegram: TelegramConnector,
        gmail: Option<GmailConnector>,
    ) -> Result<Self, EgressRegistrationError> {
        let mut declared = telegram.egress_endpoints();
        if let Some(gmail) = &gmail {
            declared.extend(gmail.egress_endpoints());
        }
        let egress_ratings = Self::build_egress_ratings(declared)?;
        let now = Timestamp::now();
        let mut runtimes = HashMap::new();
        runtimes.insert(
            "telegram".to_string(),
            ConnectorRuntime::new(
                RateLimitConfig::default(),
                CircuitBreakerConfig::default(),
                now,
            ),
        );
        if gmail.is_some() {
            runtimes.insert(
                "gmail".to_string(),
                ConnectorRuntime::new(
                    RateLimitConfig::default(),
                    CircuitBreakerConfig::default(),
                    now,
                ),
            );
        }
        Ok(Self {
            telegram,
            gmail,
            egress_ratings,
            runtimes: Mutex::new(runtimes),
        })
    }

    fn build_egress_ratings<I>(
        declared: I,
    ) -> Result<HashMap<ActionId, EgressClass>, EgressRegistrationError>
    where
        I: IntoIterator<Item = (ActionId, EgressClass)>,
    {
        let mut ratings = HashMap::new();
        for (action, class) in built_in_web_egress_endpoints() {
            Self::insert_rating(&mut ratings, action, class)?;
        }
        for (action, class) in declared {
            Self::insert_rating(&mut ratings, action, class)?;
        }
        Ok(ratings)
    }

    fn insert_rating(
        ratings: &mut HashMap<ActionId, EgressClass>,
        action: ActionId,
        class: EgressClass,
    ) -> Result<(), EgressRegistrationError> {
        match ratings.entry(action) {
            Entry::Vacant(slot) => {
                slot.insert(class);
                Ok(())
            }
            Entry::Occupied(existing) if *existing.get() == class => Ok(()),
            Entry::Occupied(existing) => Err(EgressRegistrationError {
                action: existing.key().clone(),
                existing: *existing.get(),
                requested: class,
            }),
        }
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

    /// AD-060: look up the egress class for a rated endpoint. `None` means
    /// the action is not a rated egress endpoint (gate skips the check).
    pub fn egress_class_for(&self, action: &ActionId) -> Option<EgressClass> {
        self.egress_ratings.get(action).copied()
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

    /// Admit one connector effect through its rate limiter and circuit breaker.
    #[allow(dead_code)]
    pub fn acquire_connector(
        &self,
        connector: &str,
    ) -> Result<ConnectorProbePermit, crate::connector_reality::ConnectorCallError> {
        let now = Timestamp::now();
        let runtimes = self.runtimes.lock();
        let runtime = runtimes.get(connector).ok_or_else(|| {
            crate::connector_reality::ConnectorCallError::Unavailable {
                connector: connector.to_string(),
                state: crate::connector_reality::BreakerState::Open { until: now },
            }
        })?;
        runtime.try_acquire(connector, now)
    }

    pub fn acquire_connector_with_generation(
        &self,
        connector: &str,
    ) -> Result<ConnectorProbePermit, crate::connector_reality::ConnectorCallError> {
        let now = Timestamp::now();
        let runtimes = self.runtimes.lock();
        let runtime = runtimes.get(connector).ok_or_else(|| {
            crate::connector_reality::ConnectorCallError::Unavailable {
                connector: connector.to_string(),
                state: crate::connector_reality::BreakerState::Open { until: now },
            }
        })?;
        runtime.try_acquire_with_generation(connector, now)
    }

    pub fn record_connector_outcome_for_generation(
        &self,
        connector: &str,
        permit: ConnectorProbePermit,
        succeeded: bool,
    ) {
        let runtimes = self.runtimes.lock();
        if let Some(runtime) = runtimes.get(connector) {
            runtime.record_outcome(permit, succeeded, Timestamp::now());
        }
    }

    /// Record the outcome of a connector call into its circuit breaker.
    #[allow(dead_code)]
    pub fn record_connector_outcome(&self, connector: &str, succeeded: bool) {
        let now = Timestamp::now();
        let mut runtimes = self.runtimes.lock();
        if let Some(runtime) = runtimes.get_mut(connector) {
            if succeeded {
                runtime.record_success();
            } else {
                runtime.record_failure(now);
            }
        }
    }

    #[allow(dead_code)] // introspection helper; mirrors ConnectorRuntime::breaker_state
    /// Current breaker state for a connector, if registered.
    pub fn breaker_state(&self, connector: &str) -> Option<BreakerState> {
        let runtimes = self.runtimes.lock();
        runtimes
            .get(connector)
            .map(|runtime| runtime.breaker_state())
    }
}

impl EgressClassifier for ConnectorRegistry {
    fn classify(&self, action: &ActionId) -> Option<EgressClass> {
        self.egress_class_for(action)
    }
}

#[cfg(test)]
#[path = "connectors_tests.rs"]
mod connectors_tests;
