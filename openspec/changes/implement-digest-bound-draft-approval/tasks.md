# Tasks: Implement digest-bound draft approval

## 1. Immutable draft artifact

- [ ] Define immutable draft artifact shape.
- [ ] Include body, subject, recipients, thread target, connector, and mailbox refs.
- [ ] Compute payload digest.
- [ ] Compute target digest.

## 2. Approval record

- [ ] Define approval record.
- [ ] Bind approval to owner identity.
- [ ] Bind approval to payload digest.
- [ ] Bind approval to target digest.
- [ ] Include expiry and approval channel.

## 3. Gate integration

- [ ] Mark `email.create_draft` approval-required.
- [ ] Validate approval before draft creation.
- [ ] Deny draft creation when payload digest changes.
- [ ] Deny draft creation when target digest changes.
- [ ] Keep `email.send` denied.

## 4. Gmail draft action

- [ ] Implement Gmail draft creation after approval.
- [ ] Return provider draft ID as protected/ref-safe metadata.
- [ ] Add compensating delete-draft helper if feasible.

## 5. Audit

- [ ] Audit approval.
- [ ] Audit draft creation.
- [ ] Store private payloads by protected refs/hashes only.

## 6. Tests

- [ ] Test exact approved draft can be created.
- [ ] Test body mutation invalidates approval.
- [ ] Test recipient mutation invalidates approval.
- [ ] Test thread mutation invalidates approval.
- [ ] Test send remains denied.
- [ ] Test approval audit avoids plaintext private payload.

## 7. Validation

- [ ] Run tests.
- [ ] Run `openspec validate --changes implement-digest-bound-draft-approval --strict`.
