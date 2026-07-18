//! Standing-rule artifact class (AD-010, AD-106, AD-012 leaning, AD-013).
//!
//! A standing rule is a versioned, revocable, expiring authority-composition
//! INPUT (never a second live authority object — D-007; the task grant
//! remains the only live authority object). It targets exactly one
//! [`ActionId`] and carries two independent sliding-window budgets — quota
//! (volume) and rate (velocity), per AD-106 — plus an optional dark-window
//! (AD-012 leaning) time-boxed conditional-default configuration.
//!
//! Shape mirrors [`crate::policy::Policy`]/[`crate::model_swap::ModelSwapManifest`]
//! (id/schema_version/version/lifecycle_state) so it slots into the existing
//! `artifact.propose` -> AD-142 eval-gate -> `artifact.activate` ceremony as
//! a seventh proposable kind.

use serde::{Deserialize, Serialize};

use crate::action::ActionId;
use crate::artifact::Lifecycle;
use crate::ids::ArtifactId;

/// One sliding-window budget: at most `max` uses within the trailing
/// `window_secs`. Quota (volume, e.g. 5/week) and rate (velocity, e.g.
/// 1/hour) are each one of these (AD-106).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetWindow {
    pub max: u32,
    pub window_secs: i64,
}

/// AD-012 (leaning) dark-window conditional grant: "if you don't respond in
/// `timeout_secs`, I take pre-agreed default `default`." Highest-scrutiny
/// audit case — every fire is recorded as `standing_rule.dark_window_fired`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DarkWindowConfig {
    pub timeout_secs: i64,
    pub default: DarkWindowDefault,
}

/// The pre-agreed default a dark-window timer applies when the owner does
/// not respond in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DarkWindowDefault {
    Allow,
    Deny,
}

/// The standing-rule artifact proposed via `artifact.propose { kind:
/// "standing_rule" }` and activated via `artifact.activate` after passing
/// the AD-142 offline-replay + risk-judge gate (`overlay_eval_gate`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandingRuleManifest {
    pub id: ArtifactId,
    pub schema_version: u32,
    #[serde(default = "crate::artifact::default_version")]
    pub version: u32,
    pub lifecycle_state: Lifecycle,
    /// The single action this rule authorizes without per-instance owner
    /// approval, subject to the budgets below (AD-010's composition-input
    /// invariant: this never becomes a second live authority source).
    pub action_id: ActionId,
    /// Plain-language rule statement shown to the owner at proposal time
    /// (AD-010: "agent-proposed, plain-language rules confirmed once").
    pub description: String,
    pub quota: BudgetWindow,
    pub rate: BudgetWindow,
    /// Lapse-after-unused expiry (AD-010: "e.g. lapse after 90 days
    /// unused"). Refreshed to `now + expires_after_secs` on every
    /// successful consumption; a rule that is never used lapses on its own.
    pub expires_after_secs: i64,
    #[serde(default)]
    pub dark_window: Option<DarkWindowConfig>,
}

impl StandingRuleManifest {
    /// Positive-value invariants a manifest MUST satisfy before it may ever
    /// reach owner approval or activation (P1 finding: a non-positive
    /// `window_secs` makes every trailing-window count exclude all prior
    /// usage — `now - window_secs*1e9` lands at or after `now` — so both
    /// hard caps silently admit every request; a non-positive
    /// `dark_window.timeout_secs` collapses the conditional owner-response
    /// window into an immediately-due authority path instead of failing
    /// closed). Called at proposal-parse time (`artifact_loader`) and again
    /// at activation (`Store::activate_standing_rule`) as defense in depth.
    pub fn validate(&self) -> Result<(), String> {
        if self.quota.window_secs <= 0 {
            return Err("quota.window_secs must be positive".to_string());
        }
        if self.rate.window_secs <= 0 {
            return Err("rate.window_secs must be positive".to_string());
        }
        if self.expires_after_secs <= 0 {
            return Err("expires_after_secs must be positive".to_string());
        }
        if let Some(dark_window) = self.dark_window {
            if dark_window.timeout_secs <= 0 {
                return Err("dark_window.timeout_secs must be positive".to_string());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standing_rule_manifest_round_trips_through_serde() {
        let manifest = StandingRuleManifest {
            id: "appointment_booking".to_string(),
            schema_version: 1,
            version: 1,
            lifecycle_state: Lifecycle::Proposed,
            action_id: ActionId::new("calendar.book_appointment"),
            description: "Always approve appointment bookings, up to 5/week".to_string(),
            quota: BudgetWindow {
                max: 5,
                window_secs: 7 * 24 * 3600,
            },
            rate: BudgetWindow {
                max: 1,
                window_secs: 3600,
            },
            expires_after_secs: 90 * 24 * 3600,
            dark_window: Some(DarkWindowConfig {
                timeout_secs: 1800,
                default: DarkWindowDefault::Deny,
            }),
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let back: StandingRuleManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(manifest, back);
    }

    #[test]
    fn dark_window_is_optional_and_absent_by_default() {
        let yaml = "id: no_dark_window\nschema_version: 1\nversion: 1\nlifecycle_state: proposed\naction_id: telegram.reply:owner_channel\ndescription: test\nquota: {max: 1, window_secs: 60}\nrate: {max: 1, window_secs: 60}\nexpires_after_secs: 60\n";
        let manifest: StandingRuleManifest = serde_yaml::from_str(yaml).unwrap();
        assert!(manifest.dark_window.is_none());
    }
}
