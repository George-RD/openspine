# Spec: Digest-bound approval generalization

## MODIFIED Requirements

### Requirement: The kernel MUST re-derive digests from artifact-store bytes at approval-effect time

Shell-facing request DTOs MUST structurally be unable to carry digest fields; the shell sends intents and the kernel computes outcomes. For draft approval, the kernel MUST load the content-addressed payload bytes and re-derive the payload and target semantics at effect time before allowing the effect, matching the existing D-055.4 path. For plan approval, the kernel MUST load the content-addressed plan bytes and re-derive `Plan::digest()` before persisting `ApprovalRecord`, compare it with the request-bound payload digest, and re-derive it again at resolution before recording the plan resolution. The plan target digest is kernel-authored at proposal time and remains integrity-protected by the persisted `ActionRequest`; this requirement does not claim a separate plan-target re-derivation scheme.

#### Scenario: Stored plan bytes mutate after question presentation

Given a pending plan question whose request carries the original plan digest
When approval-effect handling loads stored bytes that deserialize to a different plan
Then it MUST refuse before persisting an approval or resolving the plan.

#### Scenario: Exact stored plan bytes are approved

Given a pending plan question and content-addressed bytes whose re-derived digest matches the bound digest
When the owner taps the plan approval callback
Then the kernel MUST persist the digest-bound approval and re-run `gate()` before recording plan resolution.

#### Scenario: Shell cannot supply a plan digest

Given a shell-facing plan proposal payload
When its fields are inspected
Then it MUST NOT be able to select the approval digest independently of the canonical stored plan bytes.

#### Scenario: Payload bytes changed after approval

Given an approved `email.create_draft` request
When the kernel re-derives the payload digest from the artifact store at effect time
And the re-derived digest differs from the approved payload digest
Then the kernel MUST deny draft creation.

#### Scenario: Target digest re-derivation mismatch denies

Given an approved `email.create_draft` request
When the kernel re-derives the target digest (approval.rs:290) and it differs from the approved target digest
Then the kernel MUST deny draft creation.

#### Scenario: Shell cannot supply a digest

Given a shell-facing request DTO
When its fields are inspected
Then it MUST NOT carry a payload or target digest field
And the kernel MUST compute every digest from store bytes.

## ADDED Requirements

### Requirement: Plan approval MUST bind the complete ordered step-list digest

A plan approval MUST bind the SHA-256 digest of the complete ordered list of effectful steps, including data-handling steps. The digest MUST use the existing D-028 canonical-JSON pre-image convention and MUST be bound as the ordinary approval payload digest. The clarifying question MUST carry that digest, and an owner's affirmative response MUST approve exactly the carried digest. Existing email-body approval requirements remain unchanged.

#### Scenario: Owner approves the shown plan

Given a question is generated for a plan containing book and reminder steps
When the owner responds affirmatively
Then the resulting approval MUST carry the digest of the complete ordered step list as its approved payload digest.

#### Scenario: Data-handling step participates in the digest

Given a plan contains a step to scrub personal information before searching
When the plan digest is computed
Then that data-handling step MUST contribute to the digest exactly like every other effectful step.

### Requirement: Plan steps MUST bind exact execution identity

Each plan step MUST carry its effectful action and canonical structured arguments that fully specify the effect, including recipients, parameters, and data-handling details. A human-readable summary MAY be included for owner review but MUST NOT be the sole effect binding.

#### Scenario: Arguments change while summary remains unchanged

Given an approved step has action `calendar.book`, arguments `{time: 14:00}`, and summary `Book a slot`
When its arguments change to `{time: 15:00}` while the summary remains unchanged
Then the plan digest MUST differ.

### Requirement: A mutated approved plan MUST be refused at the gate

When an approval exists for a plan request and the current plan digest differs from the approved payload digest because a step was added, removed, reordered, or modified, `gate()` MUST deny with `ApprovalDigestMismatch` and MUST NOT return `ApprovalRequired` for a re-ask.

#### Scenario: Step is inserted after approval

Given the owner approved a plan with book and reminder steps
When a data-handling step is inserted before execution
Then `gate()` MUST deny with `ApprovalDigestMismatch`.

#### Scenario: Steps are reordered after approval

Given the owner approved a plan with ordered steps A then B
When execution presents the same steps as B then A
Then `gate()` MUST deny with `ApprovalDigestMismatch`.
