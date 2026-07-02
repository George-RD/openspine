# Design: Digest-bound draft approval

## Flow

```text
draft preview artifact
  → owner review
  → approval record(payload_digest, target_digest)
  → gate(email.create_draft)
  → connector creates Gmail draft
  → audit
```

## Immutable artifact

The draft body, recipients, subject, thread target, and connector target must be represented as an immutable artifact or immutable artifact set.

## Approval record

Approval record includes:

- approver identity;
- approval timestamp;
- task grant or workflow reference;
- payload digest;
- target digest;
- approved action;
- expiry;
- approval channel.

## Gate behavior

`email.create_draft` is approval-required.

gate() must verify approval exists and matches the exact payload/target.

## Final send

`email.send` remains denied.

Creating a draft is not equivalent to sending email, but it is still a mailbox mutation and must be approval-gated.
