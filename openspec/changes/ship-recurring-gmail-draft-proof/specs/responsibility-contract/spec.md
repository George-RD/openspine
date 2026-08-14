## MODIFIED Requirements

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

## ADDED Requirements
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
