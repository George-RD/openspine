# direct-terminal-chat Delta

## ADDED Requirements

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
