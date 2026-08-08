# direct-terminal-chat Specification

## Purpose
TBD - created by archiving change 2026-07-28-add-terminal-chat-onyx-lfm. Update Purpose after archive.
## Requirements
### Requirement: Local terminal messages SHALL use the governed owner pipeline

A message entered through `openspine chat` SHALL become a kernel-minted, locally verified owner event and traverse deterministic routing, authority composition, signed task-grant persistence, contained shell execution, model gateway mediation, action gating, and audit before a reply is displayed.

#### Scenario: Owner sends a freeform terminal message

- **WHEN** the local operator enters a non-empty message in `openspine chat`
- **THEN** OpenSpine creates a `cli.owner.message` event verified by `local_cli_auth`
- **AND** resolves the configured owner principal on an owner-device channel
- **AND** runs the terminal assistant under a persisted task grant
- **AND** displays only a reply delivered through `terminal.reply:owner_device`.

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

### Requirement: The terminal surface SHALL present owner review as an authenticated presentation/input adapter

The terminal SHALL render the single stored channel-neutral owner-review object and SHALL submit principal-bound decision intents (Approve, Reject, Narrow, Edit, Pause, Resume, Expire, Revoke, Inspect) against the same binding digest. The terminal adapter MUST NOT add scope, decisions, or lifecycle logic absent from the stored object, and MUST NOT synthesize a Telegram chat id to reuse the lifecycle. The terminal SHALL authenticate the owner through the local-owner envelope (`Source::Cli` + `LocalCliAuth`) and bind the resolved owner principal onto every decision.

#### Scenario: Terminal submits a review decision

- **WHEN** the owner issues a terminal command to decide a pending review
- **THEN** the kernel MUST authenticate the decision to the owner principal via the local-owner envelope
- **AND** bind the decision to the stored review's binding digest
- **AND** record the decision against the same stored review object a Telegram decision would record.

#### Scenario: Terminal must not fake a Telegram chat id

- **WHEN** the terminal surface submits a review decision
- **THEN** it MUST NOT synthesize or reuse a Telegram chat id to reach the lifecycle
- **AND** the decision MUST be recorded against the terminal's own owner-surface reference.

#### Scenario: Terminal cannot add scope or decisions

- **WHEN** a terminal command names a decision or a reviewed-scope value absent from the stored review object
- **THEN** the kernel MUST refuse the intent
- **AND** the refusal MUST be audit-recorded.

#### Scenario: Terminal cannot mutate lifecycle state

- **WHEN** a terminal command attempts to change a review or standing-rule lifecycle state directly
- **THEN** the kernel MUST refuse the mutation
- **AND** the refusal MUST be audit-recorded
- **AND** no lifecycle state MUST change.

#### Scenario: Terminal cannot re-derive authority

- **WHEN** a terminal command attempts to mint, widen, or re-derive a grant or authority from the review object
- **THEN** the kernel MUST refuse the attempt
- **AND** no grant or authority MUST be created or widened.

#### Scenario: A non-owner terminal principal submits a decision

- **WHEN** a terminal session that is not the bound owner principal submits a decision intent
- **THEN** the kernel MUST refuse the decision
- **AND** the review state MUST NOT change
- **AND** the refusal MUST be audit-recorded with a principal-mismatch reason.

#### Scenario: Terminal rejects a pending review

- **WHEN** the owner issues a terminal Reject for a pending review
- **THEN** the review MUST transition to `Rejected`
- **AND** no effect MAY run under the rejected review.

#### Scenario: Terminal inspects a review without mutating it

- **WHEN** the owner issues a terminal Inspect for a review
- **THEN** the kernel MUST return the stored review object
- **AND** the review state MUST NOT change
- **AND** no state-transition audit event MUST be written.

### Requirement: Owner decision code SHALL use a typed owner-surface reference, not a naked chat integer

Generic review, decision, pending-action, notification, and receipt code SHALL reference the owner via a typed `OwnerSurfaceRef` that is principal-bound, and SHALL NOT accept a naked `bound_chat_id: i64`. Connector-specific rendering ids MAY remain inside adapter storage, but generic code MUST NOT consume them. The surface reference SHALL represent at least a verified Telegram private owner chat and an authenticated local terminal/device session, and it SHALL be minted only by the adapter that authenticated the surface.

A grant's bound surface SHALL be persisted alongside the grant and SHALL be compared as a whole value wherever channel binding is enforced, so a decision arriving on a different surface — including a different channel with the same principal — is refused.

**Residual, stated deliberately rather than implied away:** the persisted surface is a sibling column (`task_grants.owner_surface_json`) and is NOT inside the grant's MAC envelope, which covers `grant_json` only. An actor with direct write access to the kernel database can therefore repoint a grant's reply surface without invalidating its MAC. This is within the existing trust boundary for that column set — the same actor can rewrite `grant_json`'s peer columns — but it is strictly weaker than MAC coverage and MUST NOT be described as authenticated. Closing it means either moving the surface inside the sealed grant or extending the MAC over the column, and is deliberately out of scope here.

#### Scenario: Generic decision code consumes a surface reference

- **WHEN** review or lifecycle code records a decision
- **THEN** it SHALL use the typed owner-surface reference bound to the principal
- **AND** it MUST NOT consume a naked chat integer from generic code.

#### Scenario: Existing Telegram grants remain valid

- **WHEN** the surface-reference seam is introduced
- **THEN** existing Telegram grants and pending rows MUST remain valid or fail closed
- **AND** a migration/compatibility test MUST prove they are not silently reinterpreted.

