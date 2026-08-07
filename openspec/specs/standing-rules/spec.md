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
The kernel MUST check quota volume and rate velocity at gate time in one immediate transaction. Saturated or failed admissions MUST NOT consume budget. Concurrent callers MUST NOT overspend either maximum. Budgets are held per standing rule: two rules that share an action but bind different reviewed scopes MUST hold independent quota and rate windows, and the kernel MUST NOT maintain an aggregate per-action counter that pools budget between them.

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

#### Scenario: Saturating one scoped rule does not spend another
- **WHEN** one of two disjoint scoped rules for the same action saturates its quota or rate
- **THEN** the other rule's remaining quota and rate MUST be unchanged
- **AND** the saturated rule MUST deny while the other continues to admit its own reviewed scope

### Requirement: Remaining budget is visible
A matched standing-rule consultation MUST return remaining quota and rate headroom in the action response. An unmatched action MUST NOT report a fabricated zero budget.

#### Scenario: Successful consultation reports decrement
- **WHEN** a matching rule admits an action
- **THEN** the response MUST expose the post-consumption quota and rate remaining
- **AND** `standing_rule_gate_response_exposes_headroom` MUST pass

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

### Requirement: Standing rules MUST bind a reviewed scope

A standing rule MUST bind the reviewed scope it was approved for, not only an action id. The binding MUST carry the required scope dimensions declared by the action's descriptor, the individual reviewed value for each of those dimensions, and a `reviewed_scope_digest` derived from those values. The rule MUST also bind the compatibility epoch of the context it was reviewed against. A rule carrying no scope binding MUST NOT be eligible for scoped admission. Two or more rules with disjoint reviewed scopes MAY be simultaneously active for one action, and each MUST hold its own independent quota and rate budget; the kernel MUST NOT pool budget between responsibilities that share an action.

#### Scenario: Reviewed scope is bound at activation

- **WHEN** a standing rule is activated for an action whose descriptor declares required scope dimensions
- **THEN** the persisted rule MUST carry every required dimension, its reviewed value, and the derived `reviewed_scope_digest`
- **AND** it MUST carry the compatibility epoch of the reviewed context
- **AND** a rule missing any required dimension MUST be rejected before activation

#### Scenario: Disjoint scoped rules coexist for one action

- **WHEN** two standing rules for `email.create_draft` are active with reviewed scopes that differ in at least one bound dimension
- **THEN** both MUST remain active
- **AND** each MUST admit only the context matching its own reviewed scope
- **AND** consuming one rule's quota or rate MUST leave the other rule's remaining budget unchanged

#### Scenario: Persisted scope binding is internally inconsistent

- **WHEN** a persisted rule's stored `reviewed_scope_digest` does not equal the digest derived from its stored dimension values
- **THEN** the binding MUST be treated as an invalid scope
- **AND** the rule MUST NOT match on either the stored digest or the stored values
- **AND** admission MUST fall back to ordinary owner approval

### Requirement: Exactly one compatible scoped rule MUST match before any budget moves

Scoped admission MUST select over every rule active for the resolved context's action. A rule is compatible only when BOTH its bound compatibility epoch AND its reviewed scope equal those of the freshly resolved context. Selection MUST complete before quota or rate is reserved and before any dark-window timer is scheduled. Exactly one compatible rule MUST admit the action. Zero compatible rules MUST fall back to ordinary owner approval. Two or more compatible rules MUST fail closed: the kernel MUST NOT break the tie by recency, narrowness, or ordering, because any such rule is a policy the owner never reviewed. Neither a mismatch nor an ambiguous overlap may consume quota or rate, create a reservation row, or mint a pending dark-window exception. The selected rule identity MUST be bound inside the same immediate transaction that reserves its budget, so a concurrent activation cannot swap the rule between selection and reservation.

#### Scenario: Exactly one rule matches

- **WHEN** one active rule's compatibility epoch and reviewed scope both equal the resolved context's
- **THEN** that rule MUST admit the action
- **AND** its quota and rate MUST be reserved only after selection completed
- **AND** the admission evidence MUST record the admitting rule id, its version, and both bound digests

#### Scenario: No rule matches the resolved scope

- **WHEN** no active rule for the action has a reviewed scope equal to the resolved context's
- **THEN** admission MUST fall back to ordinary owner approval
- **AND** no reservation row MUST exist
- **AND** no dark-window timer MUST be scheduled

#### Scenario: Ambiguous overlap fails closed

- **WHEN** two active rules for one action both match the same resolved context
- **THEN** scoped admission MUST be refused
- **AND** no quota or rate MUST be consumed and no reservation row MUST exist
- **AND** no dark-window timer MUST be scheduled and no pending exception MUST be minted
- **AND** the action MUST fall back to ordinary owner approval
- **AND** the refusal MUST leave durable owner-actionable evidence that two approved responsibilities collide

#### Scenario: Two accounts cannot form one pattern

- **WHEN** two resolved contexts for one action differ only in bound account identity, connector instance, counterparty, or canonical target
- **THEN** their `reviewed_scope_digest` values MUST differ
- **AND** a rule reviewed for one MUST NOT admit the other
- **AND** approval evidence gathered against one MUST NOT support the other

### Requirement: Bound-context drift MUST restore ordinary approval before any effect

A rule MUST stop matching when the freshly resolved context's compatibility epoch or reviewed scope no longer equals the values the rule bound — covering descriptor, implementation, executor, resolver, or policy version change, and connector instance, account identity, counterparty, canonical target, workflow, or task-shape change. Because selection precedes reservation and effect, the fallback to ordinary owner approval MUST happen before the effect runs. The kernel MUST NOT remap a rule onto a successor connector or account; an unresolvable connector or account MUST be a construction failure rather than a substitution.

#### Scenario: An instance dimension changes

- **WHEN** the resolved context's bound account identity, connector instance, counterparty, canonical target, workflow, or task shape changes after a rule was reviewed
- **THEN** the rule MUST stop matching
- **AND** the action MUST require ordinary owner approval before any effect
- **AND** no effect MUST run under the stale rule

#### Scenario: A declaration axis changes

- **WHEN** the descriptor, implementation, executor, resolver, or delegation policy version changes after a rule was reviewed
- **THEN** the bound compatibility epoch MUST no longer match
- **AND** the rule MUST stop matching and ordinary owner approval MUST be restored before effect

#### Scenario: Reviewed connector or account can no longer be resolved

- **WHEN** the connector instance or account the rule was reviewed against can no longer be resolved
- **THEN** resolved-context construction MUST fail closed
- **AND** the kernel MUST NOT substitute a successor connector or account

### Requirement: Scope-matched admission MUST map the effect outcome onto the reservation

Scope-matched admission holds a reserved budget across the effect, so the reservation decision MUST follow the executor's truthful outcome. `Executed` MUST finalize the reservation. `DeliveryUnknown` MUST retain the reservation and leave the reconciliation fence open, because releasing budget for a write that may have landed would under-count real effects. `RefusedPreEffect`, `FailedAfterAttempt`, and a missing-executor dispatch failure MUST cancel the reservation without consuming budget. A fired one-use Allow token MUST continue to be re-armed only after its cancellation succeeds.

#### Scenario: Confirmed execution finalizes the reservation

- **WHEN** scope-matched admission reserves budget and the executor returns `Executed`
- **THEN** the reservation MUST be finalized as committed usage
- **AND** the remaining quota and rate MUST reflect the consumption

#### Scenario: Unknown delivery retains the reservation

- **WHEN** scope-matched admission reserves budget and the executor returns `DeliveryUnknown`
- **THEN** the reservation MUST be retained rather than cancelled or finalized
- **AND** the reconciliation fence MUST remain open
- **AND** no dispatched-success evidence MUST be recorded

#### Scenario: Refusal and post-attempt failure release the reservation

- **WHEN** scope-matched admission reserves budget and the executor returns `RefusedPreEffect` or `FailedAfterAttempt`
- **THEN** the reservation MUST be cancelled
- **AND** no quota or rate budget MUST be consumed

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

