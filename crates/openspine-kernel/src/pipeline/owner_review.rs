//! Owner-review decision handling and standing-rule resume revalidation
//! (add-channel-neutral-responsibility-review, #129).
//!
//! Every owner decision is a principal-bound `DecisionIntent` routed through
//! the kernel-verified decision path, bound to the stored review's binding
//! digest, and audit-recorded. `Inspect` is read-only and causes no state
//! transition. Pause/resume/revoke act on the standing-rule status, not on
//! `OwnerReviewState`.
//!
//! Resume is the highest-scrutiny lifecycle transition, so it reuses the
//! `artifact.reconfirm` ceremony shape: before returning a paused rule to
//! `active`, the kernel re-verifies the reviewed bytes and binding digest,
//! re-checks the rule is still the exact paused version, and revalidates
//! policy, descriptor, executor readiness, connector/account health, and the
//! reviewed scope. A failed resume leaves the rule `paused`, requires a new
//! reviewed version, and writes a distinct audit event per rejection reason.

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::standing_rule::StandingRuleManifest;

use super::AppState;

/// The reason a resume was refused, mapped to a distinct audit event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResumeRefusal {
    NotPaused,
    Expired,
    Superseded,
    ScopeDrift,
    Unavailable,
    InvalidScope,
}

impl ResumeRefusal {
    pub(crate) fn audit_kind(self) -> &'static str {
        match self {
            ResumeRefusal::NotPaused => "standing_rule.resume_refused_not_paused",
            ResumeRefusal::Expired => "standing_rule.resume_refused_expired",
            ResumeRefusal::Superseded => "standing_rule.resume_refused_superseded",
            ResumeRefusal::ScopeDrift => "standing_rule.resume_refused_scope_drift",
            ResumeRefusal::Unavailable => "standing_rule.resume_refused_unavailable",
            ResumeRefusal::InvalidScope => "standing_rule.resume_refused_invalid_scope",
        }
    }
}

/// Revalidate a paused rule before resume and, if compatible, return it to
/// `active`. Returns `Ok(true)` when the rule was resumed, `Ok(false)` when
/// it was refused (with a distinct audit event per reason), and `Err` only on
/// a genuine store failure.
///
/// The caller MUST have already verified the submitting principal is the
/// bound owner and that the `Resume` intent is permitted.
pub(crate) fn resume_standing_rule_revalidated(
    state: &AppState,
    rule_id: &str,
    version: u32,
    now: Timestamp,
) -> Result<bool, anyhow::Error> {
    // 1. The rule must still be paused at exactly this version. An already
    //    active exact version is the replay-safe outcome of a concurrent
    //    resume and returns unchanged without a refusal audit.
    let Some(rule) = state.store.paused_standing_rule(rule_id, version)? else {
        if state.store.standing_rule_is_current(rule_id, version)? {
            return Ok(false);
        }
        let reason = match state.store.standing_rule_latest_version(rule_id)? {
            Some(latest) if latest > version => ResumeRefusal::Superseded,
            _ => ResumeRefusal::NotPaused,
        };
        refuse(state, rule_id, reason)?;
        return Ok(false);
    };

    // 2. Expiry: a paused rule that has lapsed is refused.
    let reference = rule.last_used_at.unwrap_or(rule.activated_at);
    let deadline_nanos = reference.as_nanosecond() as i64 + rule.expires_after_secs * 1_000_000_000;
    if deadline_nanos <= now.as_nanosecond() as i64 {
        refuse(state, rule_id, ResumeRefusal::Expired)?;
        return Ok(false);
    }

    // 3. Deserialize the manifest and re-verify the reviewed-scope binding.
    let manifest: StandingRuleManifest = match serde_json::from_str(&rule.rule_json) {
        Ok(m) => m,
        Err(_) => {
            refuse(state, rule_id, ResumeRefusal::InvalidScope)?;
            return Ok(false);
        }
    };
    let Some(binding) = manifest.reviewed_scope.as_ref() else {
        // A rule with no scope binding is a legacy unbounded rule; it has no
        // reviewed scope to revalidate, so it is not eligible for a
        // revalidated resume.
        refuse(state, rule_id, ResumeRefusal::InvalidScope)?;
        return Ok(false);
    };
    if !binding.binding_is_valid() {
        refuse(state, rule_id, ResumeRefusal::InvalidScope)?;
        return Ok(false);
    }

    // 4. Executor readiness: the action's descriptor AND its registered
    //    executor must both be present, or the reusable effect path is not
    //    ready and the rule must not resume.
    if !state.is_execution_backed(&rule.action_id) {
        refuse(state, rule_id, ResumeRefusal::Unavailable)?;
        return Ok(false);
    }

    // 5. Connector/account health: reuse the connector registry's breaker
    //    state; an open breaker means the connector is unavailable. No
    //    connector-specific branches in generic resume code.
    let connector = connector_for_action(state, &rule.action_id);
    if let Some(connector) = connector {
        if let Some(breaker) = state.connectors.breaker_state(connector.as_str()) {
            if breaker != crate::connector_reality::BreakerState::Closed {
                refuse(state, rule_id, ResumeRefusal::Unavailable)?;
                return Ok(false);
            }
        }
    }

    // 6. Compatibility-epoch revalidation: the rule's bound compatibility
    //    digest must still equal the catalog's current declaration axes
    //    (descriptor, implementation, executor, resolver, policy, egress,
    //    output channels). A drift on any declaration axis is a
    //    compatibility change that requires a new reviewed version.
    let Some(current_epoch) = state
        .action_catalog
        .compatibility_digest_for(&rule.action_id)
    else {
        refuse(state, rule_id, ResumeRefusal::Unavailable)?;
        return Ok(false);
    };
    if rule.compatibility_digest.as_ref() != Some(&current_epoch) {
        refuse(state, rule_id, ResumeRefusal::ScopeDrift)?;
        return Ok(false);
    }

    // 7. Reviewed-scope binding validity: the persisted binding must be
    //    internally consistent (stored values agree with the stored digest).
    //    A corrupt binding is an invalid scope and must not resume.
    if !binding.binding_is_valid() {
        refuse(state, rule_id, ResumeRefusal::InvalidScope)?;
        return Ok(false);
    }

    // 8. All checks passed: atomically flip the status and write the audit.
    state
        .store
        .resume_standing_rule(rule_id, version)
        .map_err(anyhow::Error::from)
}

fn refuse(state: &AppState, rule_id: &str, reason: ResumeRefusal) -> Result<(), anyhow::Error> {
    state
        .store
        .append_audit(
            reason.audit_kind(),
            None,
            None,
            Some(rule_id),
            None,
            &[],
            &[],
        )
        .map(|_| ())
        .map_err(anyhow::Error::from)
}

/// The connector a standing-rule action binds to, for breaker-state health
/// checks. Read from the action's implementation descriptor
/// (`connector_kind`), so a second connector becomes eligible by registering a
/// descriptor — there is no protocol branch here, which is what design.md's
/// "no connector-specific branches in generic resume code" requires. An action
/// with no implementation descriptor has no connector to health-check.
fn connector_for_action(state: &AppState, action: &ActionId) -> Option<String> {
    Some(
        state
            .action_catalog
            .implementation_descriptor_for_action(action)?
            .connector_kind
            .clone(),
    )
}
