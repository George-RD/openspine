# standing-rules Specification

## Purpose
TBD - created by archiving change implement-standing-rules. Update Purpose after archive.
## Requirements
### Requirement: Standing rules are reviewed composition inputs
The kernel MUST represent a standing rule as a versioned, revocable, expiring artifact. A rule MUST enter live consultation only after the existing proposal, evaluation, owner-approval, and activation ceremony. A standing rule MUST NOT replace or widen the authenticated task grant.

#### Scenario: Reviewed rule becomes a live input
- **WHEN** an approved standing-rule artifact is activated
- **THEN** a matching action that otherwise requires approval MAY be admitted within the rule budget
- **AND** the task grant remains the live authority object
- **AND** `standing_rule_activation_ceremony_reaches_live_consultation` MUST pass

#### Scenario: Revoked or expired rule is absent
- **WHEN** a rule is revoked or lapses after its expiry interval
- **THEN** live consultation MUST return no matching rule
- **AND** normal owner approval remains required
- **AND** `artifact_revoke_dispatch_removes_rule_from_live_consultation` and `standing_rule_lapses_after_expiry_unused` MUST pass

### Requirement: Quota and rate are independent atomic boundaries
The kernel MUST check quota volume and rate velocity at gate time in one immediate transaction. Saturated or failed admissions MUST NOT consume budget. Concurrent callers MUST NOT overspend either maximum.

#### Scenario: Quota reaches its hard cap
- **WHEN** successful admissions reach the quota maximum within its window
- **THEN** the next admission MUST be denied without recording usage
- **AND** `consult_and_reserve_atomic_budget_saturates_after_max_uses` MUST pass

#### Scenario: Rate reaches its hard cap
- **WHEN** successful admissions reach the rate maximum within its window
- **THEN** the next admission MUST be denied even when quota remains
- **AND** `standing_rule_rate_window_saturates_independent_of_quota` MUST pass

#### Scenario: Concurrent admission cannot overspend
- **WHEN** concurrent callers race for the final unit of budget
- **THEN** exactly one caller MUST consume that unit
- **AND** `standing_rule_concurrent_final_unit_race_exactly_one_wins` MUST pass

### Requirement: Remaining budget is visible
A matched standing-rule consultation MUST return remaining quota and rate headroom in the action response. An unmatched action MUST NOT report a fabricated zero budget.

#### Scenario: Successful consultation reports decrement
- **WHEN** a matching rule admits an action
- **THEN** the response MUST expose the post-consumption quota and rate remaining
- **AND** `standing_rule_gate_response_exposes_headroom` MUST pass

### Requirement: Drift requires re-review
Repeated saturation across calibrated rate windows MUST move the rule out of live consultation and surface durable audit evidence for owner re-review. The kernel MUST NOT silently widen the rule.

#### Scenario: Repeated saturation retires live consultation
- **WHEN** three distinct calibrated rate windows saturate
- **THEN** the rule MUST transition to `needs_review`
- **AND** subsequent consultation MUST fall back to normal owner approval
- **AND** `standing_rule_drift_saturates_needs_review` MUST pass

### Requirement: Dark-window defaults are durable and replay-safe
An optional dark-window default MUST be represented by a standing rule plus a durable kernel timer. Scheduling MUST be idempotent per rule. Timer replay MUST NOT grant duplicate waivers. An allow default MAY grant exactly one waiver; a deny default MUST grant none.

#### Scenario: Allow default grants one waiver
- **WHEN** an over-budget allow-default timer fires
- **THEN** exactly one subsequent admission MAY consume the waiver
- **AND** `fired_allow_token_is_digest_bound_and_one_use` MUST pass
 

#### Scenario: Deny default grants no waiver
- **WHEN** an over-budget deny-default timer fires
- **THEN** subsequent admission MUST remain denied
- **AND** `deny_default_never_dispatches_and_is_terminal` MUST pass
 

#### Scenario: Timer replay does not double grant
- **WHEN** the same fired timer event is delivered repeatedly
- **THEN** the default MUST be applied at most once
- **AND** `standing_rule_timer_redelivery_dispatches_once` MUST pass
 

#### Scenario: Repeated consultation schedules one timer
- **WHEN** over-budget requests with the same stable identity are consulted repeatedly before firing
- **THEN** exactly one pending dark-window timer MUST exist for that request identity
- **AND** `scheduling_is_idempotent_across_terminal_resolution` MUST pass

#### Scenario: Stable request identity deduplicates terminal rows
- **WHEN** the same rule, version, grant, action, chat, and encrypted payload reference are scheduled again after resolution
- **THEN** no second pending row or timer MUST be created
- **AND** `scheduling_is_idempotent_across_terminal_resolution` MUST pass

#### Scenario: Owner resolution controls the pending action
- **WHEN** the owner taps Allow or Deny before the timer fires
- **THEN** the first resolution MUST win and the timer MUST honor it
- **AND** `owner_resolution_before_fire_controls_claim` MUST pass

#### Scenario: Fired token is digest-bound and one-use
- **WHEN** an Allow default is claimed for one request fingerprint
- **THEN** a different fingerprint MUST be rejected and the matching token MUST be consumable once
- **AND** `fired_allow_token_is_digest_bound_and_one_use` MUST pass

#### Scenario: Failed effects release reservations
- **WHEN** an admitted effect fails before completion
- **THEN** its reserved quota and rate rows MUST be cancelled rather than committed
- **AND** `consult_and_reserve_cancel_leaves_headroom_unchanged` MUST pass
- **AND** `standing_rule_read_failure_cancels_reservation_no_leak`, `standing_rule_effective_allow_audit_failure_cancels_reservation`, and `standing_rule_fired_path_audit_failure_rearms_token_once` MUST pass

#### Scenario: Invalid persisted recovery data fails closed
- **WHEN** a recoverable pending row contains an invalid grant id or payload reference
- **THEN** recovery MUST return an error and MUST NOT fabricate an identity
- **AND** `decode_missing_payload_ref_propagates_error` and `recovery_surfaces_claimed_and_propagates_missing_none_payload` MUST pass

