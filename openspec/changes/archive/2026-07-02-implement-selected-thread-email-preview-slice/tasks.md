# Tasks: Implement selected-thread email preview slice

## 1. Gmail connector skeleton

- [x] Add Gmail / Google Workspace owner-mailbox connector skeleton.
- [x] Configure minimal OAuth scope for selected-thread read where possible.
- [x] Ensure send scope is not used unless unavoidable and denied by policy.

## 2. Selection token

- [x] Define selected-thread token.
- [x] Implement trusted owner selection path or controlled test stub.
- [x] Bind token to thread ID, owner, connector, and expiry.

## 3. Event and route

- [x] Normalize selected thread into `email.thread.selected`.
- [x] Add selected-thread email route artifact.
- [x] Add `email_reply_drafter` manifest.
- [x] Add selected-thread email draft workflow.
- [x] Add selected-thread email draft capability pack.

## 4. Email read

- [x] Implement selected-thread read.
- [x] Exclude attachments.
- [x] Deny inbox-wide read.
- [x] Deny unselected thread read.

## 5. Model gateway

- [x] Route private email context through model gateway.
- [x] Wrap email content as untrusted quoted data.
- [x] Store prompt/output as protected artifact refs where applicable.

## 6. Preview

- [x] Create draft preview artifact.
- [x] Present preview to owner.
- [x] Ensure no email send occurs.

## 7. Tests

- [x] Test valid selection token allows selected-thread read.
- [x] Test direct shell-provided thread ID is denied.
- [x] Test inbox-wide read is denied.
- [x] Test attachments are denied.
- [x] Test prompt injection text is treated as data.
- [x] Test private model call goes through gateway.
- [x] Test email send is denied.

## 8. Validation

- [x] Run tests: `cargo test --workspace -- --test-threads=2` (196 passed, 0 failed).
- [x] Run `openspec validate --changes implement-selected-thread-email-preview-slice --strict` (passed).
