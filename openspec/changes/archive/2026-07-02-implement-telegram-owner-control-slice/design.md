# Design: Telegram owner control slice

## Flow

```text
Telegram update
  → Telegram connector
  → owner ID verification
  → event envelope: telegram.owner.message
  → identity resolution
  → deterministic route: owner_telegram_main_assistant
  → authority composition
  → owner-control task grant
  → main_assistant_agent
  → gate-mediated status/setup/proposal actions
  → Telegram reply
  → audit
```

## Verification

Only the configured Telegram owner user ID qualifies for owner-control routing.

Handles, names, phone numbers, and message text are not sufficient.

## Main assistant authority

The first main assistant grant should allow only:

- `openspine.status.read`;
- `workflow.invoke:approved`;
- `artifact.propose`;
- `setup.workflow.start`;
- `memory.read:owner_preferences_limited` if implemented;
- `model.generate:approved_provider` through gateway when private context applies;
- `telegram.reply:owner_channel`.

Denied:

- email inbox read;
- unselected email read;
- email send;
- attachments;
- raw network egress;
- host filesystem;
- vault secret read;
- direct policy modification;
- infrastructure deploy/rollback/secret modification.

## Secret intake

If setup needs Telegram bot token or future connector secrets, secrets must bypass ordinary model/chat context.

This slice may use environment variables first and defer secret-intake implementation if clearly documented.

## Polling vs webhook

Initial implementation may use polling for simplicity.

Webhook can be added later as a separate change if needed.
