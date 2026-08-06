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

### Requirement: The terminal assistant SHALL route setup to the CLI and refuse credential intake

The terminal assistant's prompt template SHALL ground the assistant in the
CLI setup surface: `openspine setup`, `openspine setup --check`,
`openspine provider login <provider>`, and the in-chat `/status` and `/help`
commands. The template SHALL instruct the assistant never to request or
accept API keys, client ids, secrets, or tokens in conversation, and to
treat a pasted secret as exposed, because chat text enters model context
(D-014).

#### Scenario: Shipped template grounds the setup surface

- **WHEN** the kernel loads the shipped `artifacts/lyra` package
- **THEN** the `owner_terminal_template` system preamble names
  `openspine provider login`, `openspine setup --check`, `/status`, and
  `/help`
- **AND** instructs the assistant never to request or accept credentials in
  conversation.

### Requirement: An explicit local login command SHALL trigger provider OAuth

Interactive `openspine chat` SHALL accept `/login` and `/login <provider>` as
kernel-local commands. `/login` SHALL default to the only offered OAuth
provider id. The command SHALL create no kernel event, task grant, model call,
or persisted conversation row, and model output SHALL never be able to start
the flow.

#### Scenario: Owner triggers login from chat

- **WHEN** the owner enters `/login` at the interactive prompt
- **THEN** the command is resolved locally to the default provider id
- **AND** no `cli.owner.message` event and no task grant is created for it.

#### Scenario: One-shot chat never interprets login

- **WHEN** `openspine chat --once "/login"` runs
- **THEN** the text is handled as an ordinary governed message
- **AND** standard output contains only the governed reply.

### Requirement: Chat SHALL release its runtime before login starts

On `/login`, the kernel SHALL stop the terminal chat HTTP server, drop the
live runtime state, and release the data-root lifetime lock and vault handles
before invoking the provider login flow, so login reacquires the lock through
the existing `openspine provider login` path. The process SHALL exit after the
login attempt; the next start loads any newly bound provider.

#### Scenario: Lifetime lock is reacquirable at login time

- **WHEN** `/login` tears down the chat runtime
- **THEN** a fresh data-root lifetime-lock acquisition succeeds
- **AND** the login flow runs with the same behavior as
  `openspine provider login <provider>`.

