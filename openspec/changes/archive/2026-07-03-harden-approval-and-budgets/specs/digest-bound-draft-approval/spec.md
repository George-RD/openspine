# Spec: Digest-bound draft approval

## ADDED Requirements

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
