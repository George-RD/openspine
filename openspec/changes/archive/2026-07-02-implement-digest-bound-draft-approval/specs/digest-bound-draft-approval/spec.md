# Spec: Digest-bound draft approval

## ADDED Requirements

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
