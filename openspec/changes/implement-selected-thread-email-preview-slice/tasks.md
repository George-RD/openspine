# Tasks: Implement selected-thread email preview slice

## 1. Gmail connector skeleton

- [ ] Add Gmail / Google Workspace owner-mailbox connector skeleton.
- [ ] Configure minimal OAuth scope for selected-thread read where possible.
- [ ] Ensure send scope is not used unless unavoidable and denied by policy.

## 2. Selection token

- [ ] Define selected-thread token.
- [ ] Implement trusted owner selection path or controlled test stub.
- [ ] Bind token to thread ID, owner, connector, and expiry.

## 3. Event and route

- [ ] Normalize selected thread into `email.thread.selected`.
- [ ] Add selected-thread email route artifact.
- [ ] Add `email_reply_drafter` manifest.
- [ ] Add selected-thread email draft workflow.
- [ ] Add selected-thread email draft capability pack.

## 4. Email read

- [ ] Implement selected-thread read.
- [ ] Exclude attachments.
- [ ] Deny inbox-wide read.
- [ ] Deny unselected thread read.

## 5. Model gateway

- [ ] Route private email context through model gateway.
- [ ] Wrap email content as untrusted quoted data.
- [ ] Store prompt/output as protected artifact refs where applicable.

## 6. Preview

- [ ] Create draft preview artifact.
- [ ] Present preview to owner.
- [ ] Ensure no email send occurs.

## 7. Tests

- [ ] Test valid selection token allows selected-thread read.
- [ ] Test direct shell-provided thread ID is denied.
- [ ] Test inbox-wide read is denied.
- [ ] Test attachments are denied.
- [ ] Test prompt injection text is treated as data.
- [ ] Test private model call goes through gateway.
- [ ] Test email send is denied.

## 8. Validation

- [ ] Run tests.
- [ ] Run `openspec validate --changes implement-selected-thread-email-preview-slice --strict`.
