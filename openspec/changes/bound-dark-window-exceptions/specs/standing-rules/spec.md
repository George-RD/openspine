# Standing rules

## ADDED Requirements

### Requirement: Outstanding dark-window exceptions MUST be atomically bounded per reviewed rule

A dark-window configuration MUST carry a reviewed `max_pending_exceptions` limit, defaulting to one and validated to a small hard maximum, so the owner sees the bound they are approving. The kernel MUST count the nonterminal pending exceptions for the exact `(rule_id, rule_version)` inside the same immediate transaction that would schedule a new one, and MUST refuse to schedule once that count has reached the limit. A refusal MUST create no pending row, no timer, and no scheduled-default evidence. Same-request deduplication MUST be evaluated before the count, so an idempotent repeat of an already-scheduled request never consumes a slot. Concurrent callers MUST NOT both take the final slot. Varying the payload, target, grant, or owner-surface chat identifier MUST NOT increase the number of exceptions a rule version can hold.

#### Scenario: Many distinct requests yield one exception

- **WHEN** a rule with the default limit of one is over budget and many requests differing in payload, grant, and bound chat id are consulted
- **THEN** exactly one live pending exception MUST exist for that rule version
- **AND** every later request MUST leave the decision at ordinary owner approval

#### Scenario: A distinct request at the cap changes nothing

- **WHEN** a second distinct over-budget request is consulted while the limit is already reached
- **THEN** no pending row and no timer MUST be created
- **AND** no quota or rate budget MUST be consumed
- **AND** the response MUST NOT report that a default is pending

#### Scenario: Concurrent requests cannot cross the final slot

- **WHEN** two callers race to schedule the last available exception slot
- **THEN** exactly one MUST succeed
- **AND** the pending row count for that rule version MUST NOT exceed the limit

#### Scenario: Repeating one request consumes no second slot

- **WHEN** the same over-budget request is consulted repeatedly while its exception is open
- **THEN** the existing pending row MUST be reused
- **AND** no second slot MUST be consumed even when the limit is greater than one

#### Scenario: A resolved exception frees its slot

- **WHEN** an open pending exception is resolved by the owner, by its fired default, or by staleness
- **THEN** it MUST no longer count as outstanding
- **AND** the rule version MAY schedule a further exception within its limit

### Requirement: A fired dark-window exception MUST be accounted as an exception, not as quota

A fired dark-window default is an explicit exception to the reviewed budget, not additional quota. The kernel MUST record each fired exception under a distinct audit class, MUST count the allowance per `(rule_id, rule_version)` without pooling it across rules or reviewed scopes, and MUST NOT refresh the rule's lapse-after-unused clock on a fired exception. A fired exception MUST remain one-use and MUST NOT be replayable.

#### Scenario: A fired exception is audited distinctly

- **WHEN** an over-budget allow-default timer fires and its waiver is admitted
- **THEN** durable evidence MUST record it under the exception audit class
- **AND** that evidence MUST be distinguishable from ordinary in-budget admission evidence

#### Scenario: Silence does not extend a rule's life

- **WHEN** a rule's only recent activity is a fired dark-window exception
- **THEN** its lapse-after-unused clock MUST NOT be refreshed
- **AND** the rule MUST still lapse at its expiry interval

#### Scenario: Exception allowances are never pooled

- **WHEN** two rules with disjoint reviewed scopes are active for one action and one fires an exception
- **THEN** the other rule's exception allowance MUST be unchanged

### Requirement: Pending dark-window exceptions MUST be staled by every lifecycle change

Revocation, expiry, the transition to `needs_review`, and activation of a higher rule version MUST mark every unresolved pending exception for the affected rule version stale, in the same transaction as the transition that caused it. A stale exception MUST grant no authority when its timer fires and MUST NOT be re-opened by recovery. A fired token MUST additionally revalidate, before granting authority, that the reviewed scope and compatibility epoch it was minted against still equal the freshly resolved context's.

#### Scenario: Revocation stales open exceptions

- **WHEN** a standing rule is revoked while it holds an unresolved pending exception
- **THEN** that exception MUST be marked stale before any timer can claim it
- **AND** the fired timer MUST grant no authority

#### Scenario: A superseded version leaves nothing fireable

- **WHEN** a higher version of a standing rule is activated
- **THEN** every unresolved pending exception bound to the prior version MUST be marked stale

#### Scenario: Drift stales open exceptions

- **WHEN** repeated saturation moves a rule to `needs_review` while it holds an unresolved pending exception
- **THEN** that exception MUST be marked stale
- **AND** ordinary owner approval MUST be required for the next request

#### Scenario: A drifted context cannot spend a pre-drift waiver

- **WHEN** a fired token is presented and the freshly resolved context's reviewed scope or compatibility epoch no longer equals the values the pending exception bound
- **THEN** the token MUST grant no authority
- **AND** no quota or rate budget MUST be consumed

#### Scenario: Recovery does not re-open terminal slots

- **WHEN** the kernel restarts with stale and resolved pending rows present
- **THEN** recovery MUST NOT re-open them
- **AND** the outstanding exception count MUST stay within the reviewed limit

## MODIFIED Requirements

### Requirement: Dark-window defaults are durable and replay-safe

An optional dark-window default MUST be represented by a standing rule plus a durable kernel timer. Scheduling MUST be idempotent per rule and MUST be bounded by the rule's reviewed `max_pending_exceptions` limit. Timer replay MUST NOT grant duplicate waivers. An allow default MAY grant exactly one waiver; a deny default MUST grant none. A scheduling attempt refused at the exception limit MUST leave the action at ordinary owner approval, MUST create no pending row or timer, and MUST NOT report a pending default to the caller. When a standing-rule admission reaches dispatch and no runnable executor exists, the missing-executor failure MUST cancel the reservation and consume no quota or rate budget. A fired one-use Allow token MUST be re-armed only after that cancellation succeeds.

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

The cancellation-failure case is now covered: when
`cancel_standing_rule_reservation` fails, the fired token MUST stay claimed
(`token_consumed_at` non-NULL) and the reserved budget row MUST survive, so
recovery surfaces the pending row fail-closed and the one-use token can never
be spent twice. A store fault-injection hook
(`fail_next_reservation_cancel_for_test`) forces that failure.
`standing_rule_fired_path_audit_failure_rearms_token_once` injects an
*effective-Allow audit* failure and then asserts the token IS re-armed after a
successful cancel, so it exercises the opposite branch and MUST NOT be cited as
evidence for this one.

Test: `fired_token_cancel_failure_does_not_rearm_the_token`

#### Scenario: Scheduling is refused at the exception limit

- **WHEN** an over-budget request would schedule a dark window for a rule version that already holds its reviewed limit of outstanding exceptions
- **THEN** no pending row and no timer MUST be created
- **AND** the decision MUST stay at ordinary owner approval with no budget consumed
- **AND** the caller MUST NOT be told that a default is pending

#### Scenario: Invalid persisted recovery data fails closed

- **WHEN** a recoverable pending row contains an invalid grant id or payload reference
- **THEN** recovery MUST return an error and MUST NOT fabricate an identity
- **AND** `decode_missing_payload_ref_propagates_error` and `recovery_surfaces_claimed_and_propagates_missing_none_payload` MUST pass

### Requirement: Drift requires re-review

Repeated saturation across calibrated rate windows MUST move the rule out of live consultation and surface durable audit evidence for owner re-review. The kernel MUST NOT silently widen the rule. Saturation MUST be counted over both reserved and committed usage, so a rule that saturates through retained ambiguous outcomes is surfaced on the same terms as one that saturates through confirmed ones. Moving a rule to `needs_review` MUST also stale every unresolved pending dark-window exception bound to that rule version.

#### Scenario: Repeated saturation retires live consultation

- **WHEN** three distinct calibrated rate windows saturate
- **THEN** the rule MUST transition to `needs_review`
- **AND** subsequent consultation MUST fall back to normal owner approval
- **AND** `standing_rule_drift_saturates_needs_review` MUST pass

#### Scenario: Retained usage still drives the trigger

- **WHEN** a rule's saturating usage rows are retained reservations rather than committed ones
- **THEN** the drift trigger MUST still fire
- **AND** the rule MUST transition to `needs_review`
