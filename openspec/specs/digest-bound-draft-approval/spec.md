# Spec: Digest-bound draft approval

## Purpose

Bind owner approval to the exact reviewed draft payload and target so a Gmail draft can only be created from the immutable, digest-matched artifact the owner actually saw — any mutation after approval invalidates it.

## Requirements

### Requirement: Draft creation MUST require digest-bound approval

Gmail draft creation MUST require owner approval bound to the exact reviewed payload digest and target digest.

#### Scenario: Owner approves exact draft

Given the owner reviews a draft preview
And the approval record binds the payload digest and target digest
When the workflow requests `email.create_draft`
Then gate() MAY allow draft creation.

#### Scenario: Draft body changes after approval

Given the owner approved a draft body
When the draft body changes before draft creation
Then the existing approval MUST NOT authorize draft creation.

### Requirement: Target mutation MUST invalidate approval

Any recipient, subject, thread, connector, mailbox, or target mutation MUST invalidate prior approval.

#### Scenario: Recipient changes after approval

Given the owner approved a draft to one recipient
When the recipient changes before draft creation
Then the existing approval MUST NOT authorize draft creation.

### Requirement: Draft creation MUST remain approval-required

`email.create_draft` MUST remain approval-required even if it is listed as a candidate allowed action.

#### Scenario: Capability pack allows draft creation

Given a capability pack includes `email.create_draft`
When authority composition runs
Then `email.create_draft` MUST be marked approval-required unless a stricter policy denies it.

### Requirement: Final email send MUST remain denied

Digest-bound draft approval MUST NOT authorize final email send.

#### Scenario: Agent requests send after draft creation

Given a Gmail draft was created after approval
When the agent requests `email.send`
Then gate() MUST deny the request.

### Requirement: Approval audit MUST avoid plaintext private payloads

Approval audit events MUST reference payload and target digests and protected artifact refs rather than raw private email text.

#### Scenario: Approval is recorded

Given the owner approves a draft
When the approval audit event is written
Then it MUST include payload and target digests
And it MUST NOT store raw private email body as plaintext audit text.

### Requirement: Approval MUST bind only what the owner was shown

Draft approval MUST bind only the exact preview text the owner was shown
in full. If a draft preview must be truncated to fit Telegram's
message-length limit, the kernel MUST NOT propose an approval for the
underlying draft.

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

### Requirement: Approvals MUST expire

Approval records MUST carry an `expires_at`. `gate()` MUST deny a request
against an expired approval rather than treating it as still pending.

#### Scenario: Approval has expired

Given an approval record whose `expires_at` has passed
When the corresponding action is requested
Then `gate()` MUST deny the request
And MUST NOT re-ask the owner using the expired record.
