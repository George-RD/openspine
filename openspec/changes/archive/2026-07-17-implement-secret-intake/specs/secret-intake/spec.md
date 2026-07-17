# Spec: Secret intake and rotation

## ADDED Requirements

### Requirement: Connector secrets MUST be captured outside shell/model context

The kernel MUST provide an owner-triggered secret-intake mode in which the
metadata-only mode request is mediated by `gate()`, while the owner's next
verified private-channel message is captured directly by the kernel vault.
The secret value MUST NOT be included in an `ActionRequest`, task grant,
shell HTTP request/response, model input, audit payload, log, or owner-facing
confirmation.

#### Scenario: Next owner message is captured directly

Given the verified owner has entered a gated intake mode for slot `gmail.refresh`
When the owner sends the next private Telegram message containing a credential
Then the kernel MUST encrypt and store that message directly
And MUST NOT pass the message to the normal agent pipeline or model gateway
And MUST return only success/failure metadata.

### Requirement: Intake and rotation mode transitions MUST use normal gate authority

The kernel MUST expose `secret.intake` and `secret.rotate` as ordinary catalog
actions. Each mode transition MUST use a narrowly scoped, signed owner
`TaskGrant` and an `ActionRequest` containing only slot metadata, evaluated by
`gate()` with `ActionOrigin::Shell`. The actions MUST NOT be added to the
kernel-origin trusted-action set. Only `GateDecision::Allow` MAY persist a
pending capture record.

#### Scenario: Authorized intake creates pending state

Given a verified owner requests `/secret intake gmail.refresh`
When the kernel constructs the single-action owner grant and evaluates the
metadata-only request through `gate()`
Then an allowed decision MUST persist a pending record bound to the owner chat
And the record MUST contain an expiry and action/grant correlation ids
And the record MUST NOT contain the credential value.

#### Scenario: Unauthorized transition is denied

Given a request does not carry a valid, signed owner grant allowing
`secret.intake` or `secret.rotate`
When `gate()` evaluates the request
Then it MUST deny the request
And MUST NOT create pending intake state.

### Requirement: Pending captures MUST be bound and fail closed

A pending capture record MUST bind the expected private `chat_id` and a finite
expiry. Before treating a message as a credential, the kernel MUST verify both
bindings. An expired or mismatched pending record MUST be cleared, the message
MUST be discarded from normal agent/model routing, and the owner MUST receive
metadata-only retry feedback requiring a fresh gated request.

#### Scenario: Stale pending state cannot capture ordinary chat

Given pending intake state is expired or bound to another chat
When a Telegram message arrives
Then the kernel MUST NOT store that message as a secret
And MUST NOT route it to the shell or model
And MUST clear the stale state and request a fresh intake/rotation command.

### Requirement: Secret values MUST be encrypted at rest and rotatable

The kernel vault MUST encrypt every secret value at rest with AES-256-GCM
under the existing artifact root key, use a fresh nonce for each write, and
atomically replace a named slot on rotation. Startup environment values MAY
seed an absent slot exactly once, but runtime connector resolution MUST read
the vault rather than silently falling back to environment variables.

#### Scenario: Rotation replaces ciphertext without restart

Given slot `gmail.refresh` contains value A
When a gated rotation captures value B
Then the vault MUST atomically replace the encrypted slot
And a live connector instance MUST resolve B on its next call without process
restart.

### Requirement: Connectors MUST resolve credentials at call time

The Gmail and Telegram connectors MUST resolve their credential slots through
the kernel vault on every call that needs a credential. Gmail's cached
short-lived access token MUST be associated with non-secret slot versions (or
equivalent digests) and discarded whenever either credential slot changes;
Telegram MUST rebuild its Bot client when its resolved token changes.

#### Scenario: Gmail uses the rotated credentials immediately

Given a live Gmail connector cached an access token using credential versions
A1 and A2
When either stored credential rotates to version B
And the connector makes another call
Then it MUST discard the old access-token cache
And MUST refresh using the current stored credential values.

#### Scenario: Telegram adopts a rotated bot token

Given a live Telegram connector is polling with token version A
When the stored bot-token slot rotates to version B
And the connector performs its next poll or send
Then it MUST use a Bot client built with token version B without restart.

### Requirement: Secret intake outcomes MUST be metadata-only and auditable

The kernel MUST audit intake requested, intake stored, rotation requested,
rotation stored, and fail-closed rejection outcomes using slot/mode/result and
correlation ids only. No audit event, error, trace, task artifact, or owner
confirmation MAY contain the secret plaintext.

#### Scenario: Audit omits credential bytes

Given a secret value is captured and stored
When the kernel appends the intake audit event and confirms success
Then the recorded metadata MUST identify the slot and outcome
And MUST NOT contain the captured value.

### Requirement: Paired Gmail credentials MUST be staged and promoted atomically

When a Gmail credential is captured whose paired counterpart is not yet
active, the kernel MUST store the value only in a staging slot
(`secret.staged.<slot>`) together with correlation metadata
(`secret.stage.<counterpart>`) carrying a finite TTL and a correlation id
binding the two halves. The staged value MUST NOT be written to the live
slot, and the live counterpart MUST NOT be activated, until the paired
credential arrives and the connector validates the pair. Promotion to both
live slots MUST be atomic: after the first mutation, any subsequent failure
(counterpart put, staged-ciphertext delete, staging-metadata delete, or audit
append) MUST roll back the full pre-promotion snapshot — both live slots, the
staged credential value, and the staging metadata — to their prior state, and
MUST fail loudly; a failed rollback is itself a fatal error.

#### Scenario: First Gmail half is staged, not activated

Given slot `gmail.client_secret` is captured with no live `gmail.refresh_token` counterpart
When the kernel finds the paired credential is not yet present
Then it MUST store the value only in `secret.staged.gmail.client_secret`
And MUST NOT write the live `gmail.client_secret` slot.

#### Scenario: Paired promotion rolls back on any post-mutation failure

Given both a staged Gmail half and the correlated live counterpart are present
When promotion writes the candidate live slot and a later step fails
Then the kernel MUST restore both live slots, the staged credential value, and the staging metadata to their pre-promotion state
And MUST return an error rather than leaving a partially-promoted pair.

### Requirement: Telegram poll offset MUST be namespaced by bot id

The kernel MUST persist the last processed Telegram `update_id` namespaced by
the current bot id (`last_telegram_update_id.<bot_id>`). When the kernel first
learns its bot id and a legacy non-namespaced offset exists, it MUST migrate
that consumed offset into the bot-id-namespaced key exactly once and clear the
legacy key, so a same-bot token rotation does not switch from a consumed legacy offset to an empty id-key and redeliver an already-consumed (possibly secret) update.
The legacy key is cleared on that first migration, so a later different-bot
rotation finds no legacy offset to copy and therefore starts its own
namespace fresh, never replaying the previous bot's consumed updates.

#### Scenario: Same-bot rotation preserves the consumed offset

Given a consumed legacy offset `100` and a stored bot id `777`
When the poll loop resolves the next offset after a same-bot rotation
Then the namespaced key `last_telegram_update_id.777` MUST equal `100`
And an update with `update_id <= 100` MUST be filtered before reaching the pipeline.
