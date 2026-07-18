//! Nerve subscriber declarations (AD-130 / AD-132 / AD-051 / AD-112 / AD-052 / AD-034).
//!
//! A nerve is a *declared*, typed event-bus subscriber — never ad hoc code. The
//! declaration binds a subscription filter, a measure, a speak threshold, a hard
//! budget, a model tier, and a data scope no broader than the agent it advises.
//! This module is pure data + pure admission logic (no I/O); the kernel store
//! persists registrations, spends budget, and records reaction decay.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::event_bus::EventSubscriptionFilter;

/// One of the declared nerve types (AD-130).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NerveType {
    /// Legibility checker: structured objections, never better answers (AD-112).
    Advisor,
    /// Skill / workflow matcher.
    Injector,
    /// Inbound manipulation tagger (AD-034).
    Screener,
    /// Offline systemic pattern miner.
    Miner,
    /// Second-order health watcher of other nerves' interjections.
    MetaCognition,
}

/// Model tier a nerve is permitted to use. Ordered so a nerve may not exceed
/// its advisee's tier (AD-051 covert-channel guard applied to model access).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
    Cheap,
    Standard,
    Strong,
}

/// The constraint a nerve checks against (AD-130).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NerveMeasure {
    /// Advisor: missing checkable reasoning / misapplied rules (AD-112).
    Legibility,
    /// Injector: skill/workflow fit.
    SkillMatch,
    /// Screener: manipulation attempt tagging (AD-034).
    ManipulationTag,
    /// Miner: systemic pattern over the stream.
    SystemicPattern,
    /// Meta-cognition: health of other nerves' interjections.
    SecondOrderHealth,
}

/// Severity of an interjection; ordered so a speak threshold can demand a floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warn,
    Critical,
}

/// The data a nerve may observe. Open vocabulary (like `MemoryScope`): new
/// classes arrive without a schema change (D-013). A nerve is registrable only
/// when its scope is contained within its advisee's scope (AD-051).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct NerveScope {
    pub data_classes: Vec<String>,
    pub data_scopes: Vec<String>,
}

impl NerveScope {
    /// True when every class/scope in `other` is also present here — i.e. `self`
    /// is a superset of `other`. The advisee scope is the superset.
    pub fn contains(&self, other: &NerveScope) -> bool {
        other
            .data_classes
            .iter()
            .all(|c| self.data_classes.contains(c))
            && other
                .data_scopes
                .iter()
                .all(|s| self.data_scopes.contains(s))
    }
}

/// Deterministic pre-filter before any budget is spent (AD-132).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeakThreshold {
    pub severity_min: Severity,
    /// Confidence floor in `[0.0, 1.0]`.
    pub min_confidence: f64,
}

impl SpeakThreshold {
    /// Reject impossible confidence bounds before registration.
    pub fn validate(&self) -> Result<(), NerveError> {
        if !(0.0..=1.0).contains(&self.min_confidence) {
            return Err(NerveError::InvalidThreshold);
        }
        Ok(())
    }
}

/// Hard interjection budget for a nerve (AD-052). One admitted interjection
/// consumes one unit within `window_kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NerveBudget {
    pub window_kind: String,
    /// Window duration in seconds; a new window resets `suggestions_max`.
    pub window_seconds: u64,
    pub suggestions_max: u32,
}

/// A nerve declaration. Pure data; the kernel store persists it verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NerveDeclaration {
    pub id: Ulid,
    pub schema_version: u32,
    pub nerve_type: NerveType,
    pub advisee_id: String,
    /// Reuses the archived event-bus subscription filter (AD-105).
    pub subscription_filter: EventSubscriptionFilter,
    pub measure: NerveMeasure,
    pub speak_threshold: SpeakThreshold,
    pub budget: NerveBudget,
    pub model_tier: ModelTier,
    pub scope: NerveScope,
}

impl NerveDeclaration {
    /// True when the nerve's data scope is contained within `advisee_scope`
    /// (AD-051). A wider-scope nerve is unregistrable.
    pub fn is_scope_within(&self, advisee_scope: &NerveScope) -> bool {
        advisee_scope.contains(&self.scope)
    }

    /// True when the nerve's model tier does not exceed `advisee_max_tier`.
    pub fn is_tier_within(&self, advisee_max_tier: ModelTier) -> bool {
        self.model_tier <= advisee_max_tier
    }
}

/// Provenance required on every interjection (AD-052): the pattern + sources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterjectionProvenance {
    pub pattern: String,
    pub sources: Vec<String>,
}

/// A structured advisor objection (AD-112). No answer field by construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdvisorObjection {
    pub concern_class: String,
    pub cited_clause: String,
}

/// A screener manipulation tag (AD-034): detected class + the tagged aggregate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScreenerTag {
    pub manipulation_class: String,
    pub tagged_aggregate: String,
}

/// A single admitted nerve interjection. Carries structure and provenance; the
/// caller decides delivery. Cross-scope hints are `gate_visible` and structured
/// (AD-051 / AD-121), never ambient context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NerveInterjection {
    pub id: Ulid,
    pub nerve_id: String,
    pub advisee_id: String,
    pub nerve_type: NerveType,
    /// Stable decay key derived from the structured payload at admission.
    pub class: String,
    pub severity: Severity,
    pub provenance: InterjectionProvenance,
    /// True when this interjection concerns data outside the nerve's own scope
    /// and must cross as a structured, gate-visible message.
    pub gate_visible: bool,
    pub advisor: Option<AdvisorObjection>,
    pub screener: Option<ScreenerTag>,
}

/// Errors from nerve declaration, registration, and admission.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NerveError {
    #[error("nerve data scope exceeds its advisee's scope (unregistrable)")]
    ScopeExceedsAdvisee,
    #[error("nerve model tier exceeds its advisee's permitted tier")]
    TierExceedsAdvisee,
    #[error("a nerve with this id is already registered")]
    AlreadyRegistered,
    #[error("nerve not found")]
    NotFound,
    #[error("nerve interjection budget exhausted")]
    BudgetExhausted,
    #[error("this interjection class has been retired by ignored-reaction decay")]
    ClassRetired,
    #[error("interjection below the nerve speak threshold")]
    ThresholdNotMet,
    #[error("invalid speak threshold (confidence out of [0,1])")]
    InvalidThreshold,
    #[error("interjection provenance must include a pattern and at least one source")]
    InvalidProvenance,
    #[error("interjection payload does not match the declared nerve type")]
    InvalidPayload,
    #[error("store failure: {0}")]
    Storage(String),
}
/// Outcome of a reaction to a nerve interjection (AD-052 decay).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterjectionReaction {
    Engaged,
    Ignored,
    Annoyed,
}

impl NerveDeclaration {
    /// Pure admission evaluation: checks class retirement and the speak
    /// threshold (severity floor + confidence floor). Returns `Ok(())` when the
    /// interjection would be admissible; the kernel store performs the atomic
    /// budget debit and only then constructs/emits the interjection.
    pub fn evaluate_admission(
        &self,
        severity: Severity,
        confidence: f64,
        class_retired: bool,
    ) -> Result<(), NerveError> {
        if class_retired {
            return Err(NerveError::ClassRetired);
        }
        if severity < self.speak_threshold.severity_min {
            return Err(NerveError::ThresholdNotMet);
        }
        if !(confidence >= self.speak_threshold.min_confidence && confidence <= 1.0) {
            return Err(NerveError::ThresholdNotMet);
        }
        Ok(())
    }
}

/// Ignored reactions required to retire a class (AD-052 retire rule).
pub const IGNORED_RETIRE_THRESHOLD: u32 = 5;

#[cfg(test)]
mod tests {
    use super::*;

    fn decl(scope: NerveScope, threshold: SpeakThreshold, tier: ModelTier) -> NerveDeclaration {
        NerveDeclaration {
            id: Ulid::new(),
            schema_version: 1,
            nerve_type: NerveType::Advisor,
            advisee_id: "agent:advisee".to_string(),
            subscription_filter: EventSubscriptionFilter::all(),
            measure: NerveMeasure::Legibility,
            speak_threshold: threshold,
            budget: NerveBudget {
                window_kind: "task".to_string(),
                window_seconds: 3600,
                suggestions_max: 3,
            },
            model_tier: tier,
            scope,
        }
    }

    #[test]
    fn declared_types_round_trip() {
        for t in [
            NerveType::Advisor,
            NerveType::Injector,
            NerveType::Screener,
            NerveType::Miner,
            NerveType::MetaCognition,
        ] {
            let json = serde_json::to_string(&t).unwrap();
            assert_eq!(t, serde_json::from_str::<NerveType>(&json).unwrap());
        }
    }

    #[test]
    fn tiers_order_cheap_to_strong() {
        assert!(ModelTier::Cheap < ModelTier::Standard);
        assert!(ModelTier::Standard < ModelTier::Strong);
    }

    #[test]
    fn scope_containment_is_subset() {
        let advisee = NerveScope {
            data_classes: vec!["email".into(), "memory".into()],
            data_scopes: vec![],
        };
        let narrow = NerveScope {
            data_classes: vec!["email".into()],
            data_scopes: vec![],
        };
        let wide = NerveScope {
            data_classes: vec!["email".into(), "memory".into(), "secret".into()],
            data_scopes: vec![],
        };
        assert!(advisee.contains(&narrow));
        assert!(!advisee.contains(&wide));
        assert!(decl(
            narrow,
            SpeakThreshold {
                severity_min: Severity::Info,
                min_confidence: 0.0
            },
            ModelTier::Cheap
        )
        .is_scope_within(&advisee));
        assert!(!decl(
            wide,
            SpeakThreshold {
                severity_min: Severity::Info,
                min_confidence: 0.0
            },
            ModelTier::Cheap
        )
        .is_scope_within(&advisee));
    }

    #[test]
    fn threshold_below_floor_is_rejected() {
        let d = decl(
            NerveScope::default(),
            SpeakThreshold {
                severity_min: Severity::Warn,
                min_confidence: 0.9,
            },
            ModelTier::Cheap,
        );
        let err = d
            .evaluate_admission(Severity::Info, 0.95, false)
            .unwrap_err();
        assert_eq!(err, NerveError::ThresholdNotMet);
    }

    #[test]
    fn retired_class_is_rejected() {
        let d = decl(
            NerveScope::default(),
            SpeakThreshold {
                severity_min: Severity::Info,
                min_confidence: 0.0,
            },
            ModelTier::Cheap,
        );
        let err = d.evaluate_admission(Severity::Warn, 0.9, true).unwrap_err();
        assert_eq!(err, NerveError::ClassRetired);
    }
    #[test]
    fn complete_declarations_round_trip_for_all_five_types() {
        for (nerve_type, measure) in [
            (NerveType::Advisor, NerveMeasure::Legibility),
            (NerveType::Injector, NerveMeasure::SkillMatch),
            (NerveType::Screener, NerveMeasure::ManipulationTag),
            (NerveType::Miner, NerveMeasure::SystemicPattern),
            (NerveType::MetaCognition, NerveMeasure::SecondOrderHealth),
        ] {
            let mut declaration = decl(
                NerveScope {
                    data_classes: vec!["email".into()],
                    data_scopes: vec!["selected_thread".into()],
                },
                SpeakThreshold {
                    severity_min: Severity::Warn,
                    min_confidence: 0.8,
                },
                ModelTier::Cheap,
            );
            declaration.nerve_type = nerve_type;
            declaration.measure = measure;
            let json = serde_json::to_string(&declaration).unwrap();
            let decoded: NerveDeclaration = serde_json::from_str(&json).unwrap();
            assert_eq!(declaration, decoded);
            assert_eq!(decoded.subscription_filter.schema_version, 1);
            assert_eq!(decoded.budget.suggestions_max, 3);
        }
    }
}
