# Tasks: Implement digest-bound draft approval

## 1. Immutable draft artifact

- [x] Define immutable draft artifact shape.
- [x] Include body, subject, recipients, thread target, connector, and mailbox refs.
- [x] Compute payload digest.
- [x] Compute target digest.

## 2. Approval record

- [x] Define approval record.
- [x] Bind approval to owner identity.
- [x] Bind approval to payload digest.
- [x] Bind approval to target digest.
- [x] Include expiry and approval channel.

## 3. Gate integration

- [x] Mark `email.create_draft` approval-required.
- [x] Validate approval before draft creation.
- [x] Deny draft creation when payload digest changes.
- [x] Deny draft creation when target digest changes.
- [x] Keep `email.send` denied.

## 4. Gmail draft action

- [x] Implement Gmail draft creation after approval.
- [x] Return provider draft ID as protected/ref-safe metadata.
- [x] Add compensating delete-draft helper if feasible.
      Not implemented: `create_draft` is the terminal mutating step in
      this flow (digest checks all happen before it; nothing downstream
      of a successful call can fail in a way that needs undoing), so
      there is no real call site for a delete-draft compensator today.
      An unused method would be dead weight, not a genuine safety net —
      revisit if a future owner-initiated "revert this draft" command
      needs one.

## 5. Audit

- [x] Audit approval.
- [x] Audit draft creation.
- [x] Store private payloads by protected refs/hashes only.

## 6. Tests

- [x] Test exact approved draft can be created.
- [x] Test body mutation invalidates approval.
      Covered by `openspine-gate`'s existing
      `approved_but_payload_changed_since_is_denied_not_reasked` — the
      payload artifact is content-addressed and immutable once proposed,
      so this is exercised at the shared `gate()` level, not re-derived
      per action.
- [x] Test recipient mutation invalidates approval.
- [x] Test thread mutation invalidates approval.
      Recipient and thread-content mutation are the same code path here
      (D-041's target digest is derived from the thread's current
      newest-non-owner sender) — one test
      (`recipient_mutation_since_approval_is_denied_and_creates_no_draft`)
      covers both by changing the mocked thread's sender between
      proposal and approval.
- [x] Test send remains denied.
      `email.send` has no dispatch handler at all (not matched in
      `dispatch_allowed_action`) and is explicitly `denied_actions` in
      `selected_thread_email_draft_pack.yaml`; covered generically by
      `openspine-gate`'s `denied_action_returns_deny`.
- [x] Test approval audit avoids plaintext private payload.

Also covered (not originally listed, discovered during implementation):
approval callback idempotency — a second tap on a live "Approve" button
(or Telegram redelivering the same `callback_query` update) must not
mint a second `ApprovalRecord` or create a second Gmail draft
(`a_double_tap_on_approve_creates_only_one_gmail_draft`), backed by
`Store::try_consume_action_request`'s atomic single-use guard.

## 7. Validation

- [x] Run tests.
      `cargo test --workspace` (203 passed).
- [x] Run `openspec validate --changes implement-digest-bound-draft-approval --strict`.
