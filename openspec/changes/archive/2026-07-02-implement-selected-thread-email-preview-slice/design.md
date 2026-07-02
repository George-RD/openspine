# Design: Selected-thread email preview slice

## Flow

```text
Owner selects thread
  → selection token issued
  → email.thread.selected event
  → route: owner_email_selected_thread
  → authority composition
  → email_reply_drafter task grant
  → gate(email.read_thread:selected_no_attachments)
  → model gateway request
  → draft artifact
  → lyra.ui.preview / Telegram preview summary
  → audit refs
```

## Email content trust

Email content is external communication and is data, not instruction.

The model prompt must wrap email content as quoted/untrusted context.

## Selection token

The selected thread must come from a trusted owner selection path.

A shell-provided Gmail thread ID is not sufficient.

## Allowed actions

The specialist grant may allow:

- `email.read_thread:selected_no_attachments`;
- `memory.read:writing_preferences_scoped`;
- `model.generate:approved_provider` through gateway;
- `artifact.write:task_scratch`;
- `lyra.ui.preview`.

Denied:

- `email.read_inbox`;
- `email.read_thread:unselected`;
- `email.read_attachment`;
- `email.send`;
- raw network egress;
- host filesystem access;
- Telegram owner reply unless explicitly mediated by the main assistant.

## Output

Preview only.

No final send.

Draft creation is deferred to digest-bound approval in a later change.
