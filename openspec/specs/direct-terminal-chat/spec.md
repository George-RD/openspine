# direct-terminal-chat Specification

## Purpose
TBD - created by archiving change 2026-07-28-add-terminal-chat-onyx-lfm. Update Purpose after archive.
## Requirements
### Requirement: Local terminal messages SHALL use the governed owner pipeline

A message entered through `openspine chat` SHALL become a kernel-minted,
locally verified owner event and traverse deterministic routing, authority
composition, signed task-grant persistence, contained shell execution, model
gateway mediation, action gating, and audit before a reply is displayed.

#### Scenario: Owner sends a freeform terminal message

- **WHEN** the local operator enters a non-empty message in `openspine chat`
- **THEN** OpenSpine creates a `cli.owner.message` event verified by
  `local_cli_auth`
- **AND** resolves the configured owner principal on an owner-device channel
- **AND** runs the terminal assistant under a persisted task grant
- **AND** displays only a reply delivered through
  `terminal.reply:owner_device`.

### Requirement: Terminal mode SHALL NOT require Telegram credentials

Terminal chat SHALL start without reading, seeding, or requiring a Telegram
bot token.

#### Scenario: Clean local smoke test has no Telegram token

- **GIVEN** the required artifact, grant-HMAC, webhook-HMAC, and model-provider
  configuration exists
- **AND** `OPENSPINE_TELEGRAM_BOT_TOKEN` is absent
- **WHEN** the operator runs `openspine chat --once "hello"`
- **THEN** startup proceeds without Telegram polling
- **AND** the process prints one governed reply and exits.

### Requirement: Terminal reply authority SHALL be channel-specific

A terminal task grant SHALL contain `terminal.reply:owner_device` and
`terminal.owner.reply` without granting Telegram reply authority.

#### Scenario: Shell selects a reply channel

- **WHEN** the shell receives a terminal-assistant grant
- **THEN** it derives `terminal.reply:owner_device` from the allowed actions
- **AND** rejects an ambiguous grant containing both terminal and Telegram
  reply actions.

### Requirement: Conversation continuity SHALL be scoped by channel and workflow

Prompt history SHALL cross one-shot task grants only when both the persisted
bound channel id and workflow id match.

#### Scenario: Two terminal turns use separate task grants

- **WHEN** a second terminal message runs under a new task grant for the same
  owner channel and terminal workflow
- **THEN** the model prompt includes the recent first-turn exchange
- **AND** excludes turns from another channel or workflow.

