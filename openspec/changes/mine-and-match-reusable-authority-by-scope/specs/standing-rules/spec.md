# standing-rules Specification Delta

## ADDED Requirements

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

## MODIFIED Requirements

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
