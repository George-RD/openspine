# responsibility-contract Specification

## Purpose
TBD - created by archiving change define-responsibility-contract. Update Purpose after archive.
## Requirements
### Requirement: Reusable delegation MUST validate independent action and implementation declarations

The kernel MUST require a complete catalog-owned action descriptor and a complete concrete implementation descriptor before a reusable-delegation proposal reaches owner review. The implementation MUST identify a resolver and executor with explicit versions. Executor readiness MUST additionally require that the descriptor's `executor_id` is registered in the kernel-owned effect-executor registry for the action. A descriptor alone MUST NOT prove that the action is runnable. Missing or mismatched action, resolver, implementation, or executor declarations MUST fail closed.

#### Scenario: Semantic descriptor exists but executor does not

- **WHEN** `email.create_draft` has reviewed semantics but no reusable implementation descriptor
- **THEN** delegation readiness MUST return a typed missing-implementation error
- **AND** no owner proposal may claim that the reusable effect path is ready
- **AND** the typed error MUST be `MissingImplementationDescriptor`

#### Scenario: Descriptor exists but executor is not registered

- **WHEN** `email.create_draft` has reviewed semantics and a complete implementation descriptor but its declared `gmail.create_draft` executor is not registered
- **THEN** execution readiness MUST return false
- **AND** no owner proposal may claim that the reusable effect path is ready
- **AND** dispatch MUST NOT return a successful stub.

Test: `is_execution_backed_requires_descriptor_and_registered_executor`

#### Scenario: Descriptor and registered executor establish readiness

- **WHEN** `email.create_draft` has its action-keyed D-146 descriptor and the declared `gmail.create_draft` executor is registered
- **THEN** execution readiness MUST return true
- **AND** the readiness result MUST identify the descriptor-plus-registry conjunction rather than a separate effect-class enum.

### Requirement: Trusted action context MUST be resolved by the kernel

The reusable-delegation contract MUST use a kernel-resolved context containing the declared subset of connector implementation/instance, account role/identity, canonical target, bound counterparty, relationship tier, kernel-bound parameters, digests, effect classifications, workflow, and task shape. A shell MUST NOT supply or widen the trusted reviewed scope.

The kernel MUST construct that context at its own boundary — from its own connector, account, target, and counterparty resolution — before consulting any reusable-delegation input, and MUST carry the constructed context rather than a shell-supplied payload into admission. Construction MUST fail closed on a missing implementation descriptor, an unresolvable connector or account, a missing required scope dimension, or an unbound counterparty where the descriptor requires one. The generic shell dispatch path, which receives an opaque payload rather than a digest-bound request, MUST NOT reconstruct a resolved context from that payload.

#### Scenario: Counterparty is unresolved

- **WHEN** a communication action requires counterparty scope but identity resolution yields only a channel identifier
- **THEN** context construction MUST fail before owner review
- **AND** the unresolved identifier MUST NOT become reusable scope

#### Scenario: Kernel constructs the context before consulting reusable input

- **WHEN** an action with a registered implementation descriptor reaches the admission boundary
- **THEN** the kernel MUST construct the resolved context from its own resolution before any reusable-delegation input is consulted
- **AND** no field of that context may originate in shell-supplied request data

#### Scenario: Opaque shell payload cannot become a resolved context

- **WHEN** the generic shell dispatch path receives an opaque payload for an effectful action
- **THEN** it MUST NOT reconstruct a digest-bound resolved context from that payload
- **AND** it MUST continue to fail closed

### Requirement: Reviewed scope matching MUST be protocol-neutral and fail closed

A versioned reviewed action scope MUST be derived from declared generic scope dimensions. Comparison MUST return the exact changed dimensions and MUST NOT branch on protocol names. Missing required dimensions MUST fail closed.

The scope key and the compatibility epoch MUST be distinct digests. The compatibility digest is computed over declaration axes only — descriptor, implementation, executor, resolver, effect destination, required dimensions, egress class, output channels — and therefore MUST NOT be used as the scope key, because it excludes every instance axis and would collide two different accounts, targets, or counterparties into one pattern. A separate reviewed-scope digest MUST be computed over exactly the values named by the descriptor's required scope dimensions, sealed by the same canonical-JSON convention. A reviewed scope MUST persist the individual reviewed value for each required dimension alongside the derived digest, so comparison can name the exact changed dimensions and an owner can narrow one dimension without invalidating the rest. Matching MUST be exact over those sealed values: there MUST be no similarity threshold, no nearest match, and no fuzzy widening of any dimension.

#### Scenario: Synthetic connector context changes

- **WHEN** a non-Gmail synthetic context changes connector instance, account identity, target, or workflow
- **THEN** comparison MUST report those generic dimensions as mismatches
- **AND** no Gmail-specific branch may be required

#### Scenario: Persisted reviewed scope binding is corrupt

- **WHEN** the stored scope dimensions no longer match the stored context-class digest
- **THEN** scope comparison MUST return an invalid-scope outcome
- **AND** reusable execution MUST fail closed

#### Scenario: Compatibility epoch and scope key move independently

- **WHEN** two resolved contexts differ only in an instance axis such as connector instance, account identity, counterparty, canonical target, workflow, or task shape
- **THEN** their compatibility digests MUST be equal
- **AND** their reviewed-scope digests MUST differ

#### Scenario: Declaration change moves only the compatibility epoch

- **WHEN** two resolved contexts differ only in descriptor, implementation, executor, resolver, or policy version
- **THEN** their compatibility digests MUST differ
- **AND** the reviewed-scope digest MUST NOT be relied on to detect that change

### Requirement: Delegation evidence classes MUST remain distinct

Repeated approvals, explicit owner requests, correction/workflow proposals, and manually supplied artifacts MUST be distinct evidence classes. Only repeated approvals MAY support copy claiming that a pattern was observed. Repeated-approval evidence MUST be constructed from kernel-packed `OwnerApprovalEvidence`, bind at least two unique owner decision events, one owner principal, one equal request-shape digest, and a complete evidence-set digest.
Kernel-packed audit evidence MUST carry the complete `ReviewedActionScope::context_class_digest()` derived from the resolved context (the evidence/grouping key, including the generic action/descriptor class) and the separate `ResolvedActionContext::reviewed_scope_digest()` standing-rule key; the typed repeated-approval value carries only its `context_class_digest` plus the existing `OwnerApprovalEvidence` fields, with no new delegation-evidence schema. Repeated approvals MUST be grouped by the context-class digest, not by the narrower standing-rule key. Decision-event IDs MUST be deduplicated before the minimum-count check. Approvals whose context-class digests differ MUST NOT be aggregated, and a payload-digest difference alone MUST NOT reject approvals when the request-shape digest remains equal. `request_digest` is derived from `ResolvedActionContext::task_shape_digest()` and MUST exclude payload bytes, message IDs, timestamps, and decision-event IDs.

#### Scenario: Duplicate decision rows are not repeated approvals

- **WHEN** the miner receives two rows carrying the same owner decision-event ID
- **THEN** they MUST count as one decision
- **AND** repeated-approval construction MUST fail unless another unique decision exists

#### Scenario: One or duplicate approval is offered as a pattern

- **WHEN** repeated-approval evidence contains fewer than two unique owner decision events
- **THEN** construction MUST fail
- **AND** the proposal MUST NOT claim an observed pattern

#### Scenario: Non-pattern evidence is rendered

- **WHEN** provenance is constructed from an explicit owner request, correction/workflow proposal, or manually supplied artifact
- **THEN** owner-facing provenance copy MUST be derived from the structured evidence kind
- **AND** it MUST NOT claim that an observed pattern exists

#### Scenario: Approvals across two targets are not one pattern

- **WHEN** owner approvals for one action were gathered against two canonical targets whose derived context-class digests and generic reviewed-scope bindings differ
- **THEN** they MUST NOT be aggregated into a single repeated-approval evidence set
- **AND** neither target's approvals may be counted toward a pattern scoped to the other

#### Scenario: Repeated approvals belong to a different context class

- **WHEN** repeated-approval evidence carries a context-class digest different from the context class derived for the reviewed scope
- **THEN** owner-review construction MUST fail with a scope-mismatch outcome
- **AND** those approvals MUST NOT support the proposed responsibility

#### Scenario: Two context classes are not one pattern

- **WHEN** approvals for one action are gathered under two account, counterparty, or target context-class digests
- **THEN** they MUST NOT be aggregated into one repeated-approval evidence set

#### Scenario: Payload variation does not change request shape

- **WHEN** two approvals have equal request-shape digests but different payload digests
- **THEN** the repeated-approval evidence MAY remain valid
- **AND** the payload digest MUST remain a separate evidence field

#### Scenario: Provenance copy is derived from evidence

- **WHEN** repeated-approval evidence supports a proposed responsibility
- **THEN** owner-facing copy MUST be derived from the structured evidence kind
- **AND** two matching approvals MUST render `2 matching owner approvals` rather than scheduler-supplied free text

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

### Requirement: Responsibility MUST remain a reference view rather than live authority

A responsibility manifest MUST reference reviewed workflow and standing-rule artifacts and MAY record scope, limits, compatibility, provenance, status, and lifecycle controls. It MUST NOT contain a task grant, action allowlist, capability pack, or direct live executor authority. Every task MUST still receive an ordinary task grant and pass through `gate()`.

#### Scenario: Active responsibility executes another task

- **WHEN** an active responsibility is used for a later matching task
- **THEN** the runtime MUST compose and mint a fresh task grant
- **AND** the responsibility manifest itself MUST NOT authorize the effect

### Requirement: Compatibility drift or unavailable resolution MUST require re-review

A changed descriptor, implementation, policy, workflow, reviewed scope, connector availability, or account resolution MUST move the responsibility to a `needs_review` outcome. The kernel MUST NOT silently remap a responsibility to a replacement connector or account.

Drift on any bound epoch MUST restore ordinary owner approval **before** the effect. Because the compatibility and reviewed-scope comparisons run before any budget is reserved and before any effect is dispatched, there MUST NOT exist a window in which a drifted responsibility admits an effect and the drift is only observed afterwards.

#### Scenario: Connector instance disappears

- **WHEN** the reviewed connector/account context can no longer be resolved
- **THEN** compatibility assessment MUST return `needs_review`
- **AND** reusable execution MUST not continue under a guessed successor

#### Scenario: Drift is observed before the effect, not after

- **WHEN** any bound epoch or reviewed dimension of an active responsibility changes
- **THEN** the change MUST be detected before budget is reserved and before the effect is dispatched
- **AND** the action MUST require ordinary owner approval for that request
- **AND** no effect MUST run under the drifted responsibility

### Requirement: Communication dark-window Allow MUST be forbidden

Reusable delegation for communication or connector-write effects MUST reject any policy that permits a dark-window Allow default. Timeout behavior MUST remain deny or require explicit review until a future decision changes this posture.

Enforcement MUST exist on the activation path, not only in review. Dark-window Allow eligibility MUST be an explicit catalog allowlist: an action is eligible only when it is named on that allowlist, and an action the catalog does not know MUST be treated as ineligible. Eligibility MUST NOT be inferred from other declarations, because a predicate that permits whatever it cannot classify is not fail closed. The kernel MUST refuse to activate a standing rule whose dark-window default is Allow for an ineligible action, before the rule is persisted as active, leaving no active rule row and durable owner-actionable evidence naming the action. Because activation is not retroactive, the kernel MUST also converge stored state: an active rule whose stored Allow default is ineligible MUST be moved out of live consultation, with its unresolved pending exceptions staled in the same transaction. Permitting an action MUST be an explicit catalog decision with proposal-specific proof.

#### Scenario: Draft action declares bounded Allow

- **WHEN** a communication or owner-account write descriptor declares a bounded Allow dark window
- **THEN** delegation validation MUST fail before owner review

#### Scenario: Activation refuses an Allow default for an ineligible action

- **WHEN** a standing rule whose dark-window default is Allow is activated for an action the catalog does not name on the eligibility allowlist
- **THEN** activation MUST be refused
- **AND** no active rule row MUST be written
- **AND** durable evidence MUST record the refusal and the action it applied to

#### Scenario: A stored ineligible Allow rule is retired on open

- **WHEN** the kernel opens a database holding an active rule whose dark-window default is Allow for an ineligible action
- **THEN** that rule MUST be moved out of live consultation
- **AND** its unresolved pending exceptions MUST be staled
- **AND** durable evidence MUST record the retirement

#### Scenario: A Deny default stays permitted

- **WHEN** the same rule declares a Deny dark-window default instead
- **THEN** activation MUST succeed
- **AND** the timeout behaviour MUST remain deny

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

For `Resume` and `Pause`, the lifecycle adapter MUST distinguish `Resumed`/`AlreadyActive` or `Paused`/`AlreadyPaused` replays from `Refused`, map `Refused` to `LifecycleRefused` before committing the owner-review decision, and write the refusal event even when the underlying standing-rule update returns no changed row. A refused lifecycle intent MUST leave the standing rule unchanged and MUST NOT emit a successful lifecycle receipt.

#### Scenario: Edit is refused because the kernel cannot synthesise a replacement

- **WHEN** an owner submits `Edit` against a stored review
- **THEN** the kernel MUST refuse it as requiring a replacement review
- **AND** the review state MUST be unchanged

#### Scenario: Resume refusal preserves the paused rule

- **WHEN** an owner submits `Resume` for an expired, drifted, unavailable, superseded, or otherwise ineligible paused rule
- **THEN** the terminal route MUST return a typed lifecycle refusal and leave the rule paused and unchanged
- **AND** it MUST write `owner_review.decision_refused` without a successful lifecycle receipt

#### Scenario: Pause refusal preserves an ineligible non-paused rule

- **WHEN** an owner submits `Pause` for a needs-review, revoked, or otherwise non-active rule that is not already paused
- **THEN** the terminal route MUST return a typed lifecycle refusal and leave the rule unchanged
- **AND** it MUST write `owner_review.decision_refused` without a successful lifecycle receipt

#### Scenario: Pause on an already paused rule is an unchanged replay

- **WHEN** an owner submits `Pause` for an already paused rule
- **THEN** the terminal route MUST return an `AlreadyPaused` unchanged replay receipt
- **AND** it MUST NOT write a refusal or mutate the rule

Test: `terminal_review_resume_refusal_is_truthful`, `terminal_review_pause_refusal_is_truthful`

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

Review receipts MUST remain lifecycle/authority receipts rather than claims that an external effect ran; any scoped responsibility receipt is a separate effect-path receipt governed by the following requirement.

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

### Requirement: Miner-originated owner reviews MUST bind exact evaluated proposals

A repeated-approval miner proposal MUST enter the existing `artifact.propose` chain and its exact replay/risk-judge gate before owner review. The kernel MUST persist both verdicts and bind their proposal identity, artifact digest, verdict identities, and evaluation epochs into the content-addressed owner-review object and binding digest. A review MUST NOT be created, approved, or narrowed from an absent, denied, stale, differently bound, or incompatible verdict. Approval MUST reuse the evaluated proposed artifact's activation/currency path; Narrow MUST re-evaluate the narrowed proposal through the same `artifact.propose` chain before persisting a replacement review. Reject MAY dispose the pending review without activation.

#### Scenario: Miner proposal reaches owner review only after exact evaluation

- **WHEN** a repeated-approval miner proposal is dispatched
- **THEN** the kernel MUST run the same exact-proposal replay and risk-judge evaluation reached by `artifact.propose`
- **AND** the owner review MUST bind the resulting proposal digest, artifact identity, both verdicts, and their epochs

#### Scenario: Missing or stale evaluation refuses review

- **WHEN** either evaluation verdict is absent, denied, bound to another artifact/digest, or stale on a compatibility epoch
- **THEN** the kernel MUST fail closed without creating an `OwnerReviewRequest` row for a dispatch failure or approving an existing owner review for an approval-time failure
- **AND** no standing rule MUST be activated

#### Scenario: Narrowed miner review is re-evaluated

- **WHEN** an owner narrows an evaluation-bound miner review
- **THEN** the narrowed proposal MUST pass the same exact-proposal evaluation before a replacement review is persisted
- **AND** a failed or stale evaluation MUST leave the original review pending and unchanged

Test: `miner_proposal_missing_verdict_refuses_without_review_or_activation`, `miner_proposal_mismatched_verdict_refuses_without_review_or_activation`, `miner_proposal_non_review_required_refuses_without_review_or_activation`, `miner_proposal_stale_verdict_refuses_without_review_or_activation`, `miner_review_approval_denied_verdict_refuses_without_activation`, `miner_approval_refuses_review_proposal_digest_mismatch`

### Requirement: Scoped effect admission MUST emit a responsibility receipt

A scoped effective Allow that admits an effect MUST return or audit a responsibility receipt naming the admitting standing-rule id and version, the canonical target references from the kernel-resolved context, and quota/rate headroom after reservation. The receipt MUST describe authority admission only; the shared executor's `EffectOutcome` remains the source of truth for whether an external effect executed.

#### Scenario: Scoped draft admission returns responsibility

- **WHEN** `email.create_draft` matches exactly one reviewed standing rule and reserves budget
- **THEN** the action response and effective-Allow audit MUST expose the rule id, version, resolved target, and post-reservation quota/rate remaining
- **AND** the receipt MUST NOT claim Gmail created a draft before `EffectOutcome::Executed`

#### Scenario: Fallback has no scoped responsibility receipt

- **WHEN** erased, unresolved, unmatched, pending-fenced, or otherwise refused scoped admission falls back to ordinary owner approval
- **THEN** no scoped responsibility receipt MUST claim that a standing rule admitted the effect

