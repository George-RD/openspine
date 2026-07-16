# Spec: Digest-bound draft approval

## ADDED Requirements

### Requirement: The kernel MUST re-derive digests from artifact-store bytes at approval-effect time

Shell-facing request DTOs MUST structurally be unable to carry digest fields; the
shell sends intents, the kernel computes outcomes (AD-120). At approval-effect time
the kernel MUST re-derive the payload digest from the artifact-store bytes — the
target digest re-derivation already exists at
`crates/openspine-kernel/src/pipeline/approval.rs:290` — and MUST deny the effect
if the re-derived digest does not match the approved digest. The kernel MUST NEVER
trust a shell-supplied digest string. (Settles D-055.4.)

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

## MODIFIED Requirements

### Requirement: Draft creation MUST require digest-bound approval

Gmail draft creation MUST require owner approval bound to the exact reviewed payload
digest and target digest. At approval-effect time the kernel MUST re-derive both
digests from artifact-store bytes and deny on any mismatch (see "The kernel MUST
re-derive digests from artifact-store bytes at approval-effect time"); the kernel
MUST NOT rely on a shell-supplied digest.

#### Scenario: Owner approves exact draft

Given the owner reviews a draft preview
And the approval record binds the payload digest and target digest
When the workflow requests `email.create_draft`
Then gate() MAY allow draft creation.

#### Scenario: Draft body changes after approval

Given the owner approved a draft body
When the draft body changes before draft creation
Then the existing approval MUST NOT authorize draft creation.

#### Scenario: Kernel re-derivation catches the mutation

Given the approved payload digest
When the kernel re-derives the payload digest from the artifact store at effect time
Then a mismatch with the approved digest MUST deny draft creation.

### Requirement: Approval audit MUST avoid plaintext private payloads

Approval audit events MUST reference payload and target digests and protected
artifact refs rather than raw private email text. The digests recorded in the audit
MUST be the kernel-re-derived digests computed from artifact-store bytes at
effect time, never a shell-supplied digest string.

#### Scenario: Approval is recorded

Given the owner approves a draft
When the approval audit event is written
Then it MUST include payload and target digests
And it MUST NOT store raw private email body as plaintext audit text.

#### Scenario: Audit records the re-derived digest

Given the kernel re-derived payload and target digests at effect time
When the approval audit event is written
Then it MUST record those kernel-re-derived digests
And MUST NOT record a digest supplied by the shell.

### Requirement: Approval MUST bind only what the owner was shown

Draft approval MUST bind only the exact preview text the owner was shown in full.
The binding digest MUST be re-derived by the kernel from the artifact-store payload
at effect time and MUST match what the owner was shown; the target digest
re-derivation already exists at `crates/openspine-kernel/src/pipeline/approval.rs:290`.
If a draft preview must be truncated to fit Telegram's message-length limit, the
kernel MUST NOT propose an approval for the underlying draft.

#### Scenario: Preview must be truncated

Given a draft preview body that exceeds Telegram's UTF-16 message limit
When `dispatch_lyra_preview` truncates the shown text
Then the kernel MUST NOT call `propose_draft_creation`
And no `ActionRequest` for `email.create_draft` MUST be persisted
And the owner MUST see a notice that the draft is too long to approve via Telegram.

#### Scenario: Preview fits without truncation

Given a draft preview body that fits within Telegram's UTF-16 message limit
When `dispatch_lyra_preview` runs
Then the existing approval-proposal flow MUST be unchanged.

#### Scenario: Re-derived payload digest must match what was shown

Given the owner was shown a preview
When the kernel re-derives the payload digest from the artifact-store bytes at effect time
Then it MUST match the approved payload digest
And a mismatch MUST deny draft creation.
