# responsibility-contract Specification Delta

## MODIFIED Requirements

### Requirement: Owner review MUST be channel-neutral and digest-bound

One semantic owner-review object MUST carry provenance, exact reviewed scope, automatic effects, remaining boundaries, limits, fallback behavior, proposal/compatibility digests, decisions, and lifecycle controls. Transport adapters MUST render and submit against that object rather than inventing channel-specific semantics.

The review object MUST be persisted whole as a content-addressed artifact and a digest-bound review record before any owner surface may render it. Retrieval MUST re-verify the stored bytes and the binding digest before a decision is eligible. Owner-facing rendering MUST NOT add scope, decisions, or lifecycle controls absent from the stored object. An approvable review whose full owner-facing rendering would be truncated on the target channel MUST NOT be persisted as approvable.

Every owner decision MUST be a principal-bound intent routed through the kernel-verified decision path, bound to the same binding digest the owner was shown, and audit-recorded. Approve and Narrow MUST reuse the digest-bound approval guarantee (D-011 WYSIWYS): the decision binds the same binding digest the owner was shown and is re-verified at effect time; the review row is the sole disposition of the decision, and the approved effect still crosses `gate()` under a fresh task grant (D-007).

#### Scenario: Review is rendered on another owner surface

- **WHEN** the same review is rendered by Telegram, the local terminal, or a future authenticated web surface
- **THEN** every surface MUST refer to the same semantic binding digest
- **AND** the contract MUST contain no channel-specific chat or callback fields

#### Scenario: Persisted review bytes are altered

- **WHEN** any semantic review field changes without a new binding digest
- **THEN** binding validation MUST fail
- **AND** the altered review MUST NOT be eligible for an owner decision

#### Scenario: An approvable review is too long for the channel

- **WHEN** the full owner-facing rendering of an approvable review would be truncated on the target channel
- **THEN** the review MUST NOT be persisted as approvable
- **AND** the owner MUST see a notice that the review is too large to approve on that channel

#### Scenario: A surface submits a decision outside the stored object

- **WHEN** a Telegram or terminal adapter submits a decision intent that was not present in the stored review's available decisions or lifecycle controls
- **THEN** the kernel MUST refuse the decision
- **AND** the refused intent MUST be audit-recorded

#### Scenario: A decision is recorded against the reviewed digest

- **WHEN** an owner submits Approve or Narrow through either surface
- **THEN** the decision MUST bind the same binding digest the owner was shown
- **AND** the kernel MUST re-verify the digest at effect time before recording the decision

#### Scenario: A non-owner principal submits a decision

- **WHEN** a principal that is not the bound owner submits any decision intent
- **THEN** the kernel MUST refuse the decision
- **AND** the review state MUST NOT change
- **AND** the refusal MUST be audit-recorded with a principal-mismatch reason

## ADDED Requirements

### Requirement: Owner review MUST have a typed lifecycle with validated transitions

The owner-review record MUST carry an `OwnerReviewState` with a fixed set of states and a `can_transition` validator that enforces legal transitions, mirroring the `Lifecycle` enum-plus-validator idiom (`crates/openspine-schemas/src/artifact.rs:60`). The review state set is `Pending / Approved / Rejected / Narrowed / Revoked / Expired`. Pause and resume are standing-rule runtime statuses, not review states, so they are absent from `OwnerReviewState`. An expired review MUST NOT be eligible for an owner decision.

The legal transitions are:

| From | To |
| --- | --- |
| `Pending` | `Approved`, `Rejected`, `Narrowed`, `Revoked`, `Expired` |
| `Narrowed` | `Approved`, `Rejected`, `Narrowed`, `Revoked`, `Expired` |
| `Approved` | `Revoked`, `Expired` |
| `Rejected` | `Revoked`, `Expired` |
| `Revoked` | `Expired` |
| `Expired` | (terminal) |

`Revoked` is reachable from any non-terminal state through an owner `Revoke` intent submitted against the review. It is deliberately NOT triggered by the standing rule being revoked through some other path (for example `artifact.revoke`): a review is a record of an owner decision, and a rule revoked elsewhere leaves that record accurate rather than retroactively restating it as a decision the owner did not make. `Expired` is terminal. Any transition not listed MUST be refused.

#### Scenario: An illegal lifecycle transition is attempted

- **WHEN** a caller attempts a transition not listed in the legal-transition table for the current review state
- **THEN** the kernel MUST refuse the transition
- **AND** the refusal MUST be audit-recorded

#### Scenario: An expired review receives a decision

- **WHEN** an owner submits a decision for a review whose `expires_at` has passed
- **THEN** the kernel MUST refuse the decision
- **AND** the review MUST transition to the expired state

#### Scenario: A pending review is approved

- **WHEN** an owner approves a `Pending` review
- **THEN** the review MUST transition to `Approved`
- **AND** the decision MUST be recorded against the review's binding digest

#### Scenario: A pending review is rejected

- **WHEN** an owner rejects a `Pending` review
- **THEN** the review MUST transition to `Rejected`
- **AND** no effect MAY run under the rejected review

#### Scenario: A pending review is narrowed

- **WHEN** an owner narrows a `Pending` review
- **THEN** the review MUST transition to `Narrowed`
- **AND** a new review object with a new binding digest MUST be created

#### Scenario: A review is revoked

- **WHEN** the owner submits a `Revoke` intent against the review
- **THEN** the bound standing rule MUST be revoked and the review MUST transition to `Revoked`
- **AND** no further decision MAY be recorded against the revoked review

### Requirement: Decision intents MUST map onto the digest-bound decision sets

A `DecisionIntent` MUST be a total mapping onto the union of the two existing digest-bound decision sets: `OwnerReviewDecision` (`Approve`, `Reject`, `Narrow`, `Edit`) and `ResponsibilityLifecycleControl` (`Pause`, `Resume`, `Expire`, `Revoke`). Every intent except `Inspect` MUST be gated by the membership rule: a decision intent drawn from `OwnerReviewDecision` MUST be present in the review's `available_decisions`, and one drawn from `ResponsibilityLifecycleControl` MUST be present in the review's `lifecycle_controls`, before it may be submitted. `Inspect` is a read-only intent that causes no state transition and is exempt from the membership check.

#### Scenario: A decision intent is gated by the reviewed decision set

- **WHEN** an owner submits `Approve` for a review whose `available_decisions` does not contain `Approve`
- **THEN** the kernel MUST refuse the decision
- **AND** the refusal MUST be audit-recorded

#### Scenario: A lifecycle intent is gated by the reviewed control set

- **WHEN** an owner submits `Pause` for a review whose `lifecycle_controls` does not contain `Pause`
- **THEN** the kernel MUST refuse the decision
- **AND** the refusal MUST be audit-recorded

#### Scenario: Inspect is read-only and causes no transition

- **WHEN** an owner submits `Inspect` for a review
- **THEN** the kernel MUST return the stored review object
- **AND** the review state MUST NOT change
- **AND** no audit event recording a state transition MUST be written

### Requirement: Decision refusals MUST be typed and enumerated

Every refusal on the decision path MUST be a typed reason rather than a generic error, so a surface can report why without inventing copy. The enumerated refusals are: the submitting principal is not the review's bound owner principal; the submitted binding digest is not the stored review's; the review has expired; the intent fails the membership check; the intent is not legal from the review's current state; `Edit` was submitted without a replacement review, which the kernel cannot synthesise and therefore always refuses; `Narrow` was submitted without a narrowed scope, or with one that widens any reviewed dimension; `Approve` was submitted for an action whose executor is not registered; and `Resume` was submitted for a rule that is not paused. A refusal MUST leave the review row unchanged and MUST write a `owner_review.decision_refused` audit event.

#### Scenario: Edit is refused because the kernel cannot synthesise a replacement

- **WHEN** an owner submits `Edit` against a stored review
- **THEN** the kernel MUST refuse it as requiring a replacement review
- **AND** the review state MUST be unchanged

### Requirement: A digest-bound decision MAY be submitted with a short digest token

A channel whose input carries a hard length limit (Telegram's 64-byte callback data) MAY carry an unambiguous prefix of the binding digest rather than the whole digest. The kernel MUST reload the stored review, re-verify its full artifact digest and full binding digest, and MUST reject a token that does not prefix the stored digest. A short token is an input encoding, never a weaker binding.

#### Scenario: A callback token that does not match the stored digest

- **WHEN** a decision arrives carrying a digest token that is not a prefix of the stored review's binding digest
- **THEN** the decision MUST be refused before any state transition

### Requirement: Narrow MUST create a new immutable review digest

An owner's Narrow decision MUST construct a new owner-review object whose reviewed scope is narrowed and whose binding digest differs from the original, and MUST persist it as its own content-addressed record. The narrowed scope MUST remain a valid, matchable reviewed scope. The original review object MUST remain immutable, and a narrowed decision MUST NOT be replayable as approval of the broader original.

#### Scenario: Narrow narrows one reviewed dimension

- **WHEN** an owner narrows one reviewed-scope dimension of a pending review
- **THEN** the kernel MUST construct a new review object with a narrower scope and a new binding digest
- **AND** the narrowed scope MUST remain a valid reviewed scope for matching
- **AND** the original review object MUST remain unchanged

#### Scenario: A narrowed review is confused with the original

- **WHEN** a narrowed review object and its broader original are both persisted
- **THEN** their binding digests MUST differ
- **AND** a decision recorded against one MUST NOT authorize the other

### Requirement: Owner review MUST expose post-use receipts

After a review is decided, the kernel MUST emit a receipt truthful to what the decision actually committed, never restating a false success. Receipts MUST reference the review's binding digest.

A review decision commits a *lifecycle/authority* outcome, not an external effect: approving a review activates the derived standing rule, and any later effect is a separate, separately gated `ActionRequest` under a fresh task grant (D-007). A review receipt therefore MUST describe only the committed lifecycle outcome and MUST NOT claim that an external effect executed. The effect's own `EffectOutcome` truthfulness contract (#127) governs the receipt for that effect, on the effect path, and is unchanged by this change.

#### Scenario: Receipt describes only the committed lifecycle outcome

- **WHEN** an owner approves a review and the standing rule is activated
- **THEN** the receipt MUST name the review, the intent, and the binding digest it was decided against
- **AND** the receipt MUST NOT claim that any external effect was executed or delivered

#### Scenario: Receipt for a replayed decision claims no new effect

- **WHEN** a duplicate decision intent is submitted against an already-decided review
- **THEN** the receipt MUST report the decision as unchanged
- **AND** it MUST NOT report a second committed outcome

### Requirement: Proposal copy MUST NOT outrun evidence, scope, or executor readiness

Owner-facing proposal copy MUST be derived from the stored provenance and MUST NOT claim an observed pattern unless repeated-approval evidence supports it. Copy MUST name only the reviewed scope digest. Copy MUST NOT claim the reusable effect path is ready unless the action's descriptor AND its registered executor are both present.

#### Scenario: Copy claims readiness without a registered executor

- **WHEN** an action has a descriptor but no registered executor
- **THEN** rendered copy MUST NOT claim the reusable effect path is ready
- **AND** the copy MUST reflect that readiness is not established

#### Scenario: Copy is derived from non-pattern evidence

- **WHEN** provenance is an explicit owner request, correction/workflow proposal, or manually supplied artifact
- **THEN** rendered copy MUST be derived from the structured evidence kind
- **AND** it MUST NOT claim an observed pattern
