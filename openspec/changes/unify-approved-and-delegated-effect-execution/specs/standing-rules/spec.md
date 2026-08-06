# standing-rules Specification

## MODIFIED Requirements

### Requirement: Dark-window defaults are durable and replay-safe

An optional dark-window default MUST be represented by a standing rule plus a durable kernel timer. Scheduling MUST be idempotent per rule. Timer replay MUST NOT grant duplicate waivers. An allow default MAY grant exactly one waiver; a deny default MUST grant none. When a standing-rule admission reaches dispatch and no runnable executor exists, the missing-executor failure MUST cancel the reservation and consume no quota or rate budget. A fired one-use Allow token MUST be re-armed only after that cancellation succeeds.

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

#### Scenario: Missing executor cancels an ordinary consult reservation

- **WHEN** a standing-rule admission reserves quota or rate budget for `email.create_draft`
- **AND** generic dispatch reaches the action with no runnable executor
- **THEN** dispatch MUST return the typed `DispatchError::NoExecutor` failure
- **AND** the consult reservation MUST be cancelled rather than finalized
- **AND** no quota or rate budget MUST be consumed

Test: `delegated_email_draft_fails_closed_and_cancels_reservation`

#### Scenario: Fired missing executor cancels and re-arms the token

- **WHEN** a fired one-use Allow token admits `email.create_draft` during the dark window
- **AND** generic dispatch reaches the action with no runnable executor
- **THEN** dispatch MUST return the typed `DispatchError::NoExecutor` failure
- **AND** the fired reservation MUST be cancelled rather than finalized
- **AND** the database MUST show zero reserved rows and zero committed rows
- **AND** the full remaining quota and rate budget MUST be unchanged
- **AND** `token_consumed_at` MUST be NULL again

Test: `fired_token_no_executor_cancels_reservation_and_rearms_once`

The cancellation-failure case — where a failed reservation cancel MUST leave the
token claimed rather than re-armed — is pre-existing kernel behaviour and is NOT
covered by any test for `NoExecutor`. No existing test forces
`cancel_standing_rule_reservation` to fail;
`standing_rule_fired_path_audit_failure_rearms_token_once` injects an
*effective-Allow audit* failure and then asserts the token IS re-armed after a
successful cancel, so it exercises the opposite branch and MUST NOT be cited as
evidence for it. Forcing a cancel failure requires a new store fault-injection
hook and is deferred.

#### Scenario: Invalid persisted recovery data fails closed

- **WHEN** a recoverable pending row contains an invalid grant id or payload reference
- **THEN** recovery MUST return an error and MUST NOT fabricate an identity
- **AND** `decode_missing_payload_ref_propagates_error` and `recovery_surfaces_claimed_and_propagates_missing_none_payload` MUST pass
