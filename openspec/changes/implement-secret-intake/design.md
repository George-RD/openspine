# Design: Secret intake and rotation

## Approach

The kernel owns a mutable, name-addressed encrypted vault. The owner enters a
short-lived intake mode for one slot using a metadata-only `/secret` command.
The mode transition is a normal shell-origin gate decision over a narrowly
scoped, HMAC-sealed owner `TaskGrant`; the secret value is deliberately not
part of the `ActionRequest`. The next verified private Telegram message is
captured before the ordinary pipeline, encrypted immediately, and replaced
on rotation. Connectors hold a shared vault handle and resolve credentials at
the moment each external call needs them.

## Secret vault

`SecretStore` mirrors `ArtifactStore`'s AES-256-GCM construction and nonce
prefix format, but uses a `data/credentials/<validated-slot>` path rather than
content-addressed immutable blobs. Each write uses a fresh random 12-byte
nonce and write-then-rename; a read rejects truncated, undecryptable, or
invalid slot paths. The plaintext is returned only to a kernel connector call
and is never serialized into an action, audit event, tracing field, HTTP body,
or owner confirmation. `seed_if_absent` is used only at startup to migrate
first-run env bootstrap values into the vault; runtime connector resolution
reads the vault, not the environment.

`OPENSPINE_ARTIFACT_KEY` remains the vault root-key bootstrap. It cannot be
intaked into the vault because the process needs it before the vault can be
opened. The Telegram token and Gmail OAuth values may be seeded once from
legacy env variables, then are runtime-vault values and can be rotated.
Provider API-key migration is owned by the foundation-amendment lane.

## Intake state machine

The accepted command forms are `/secret intake <slot>` and `/secret rotate
<slot>`. Slot names are bounded and path-safe. The parser rejects malformed
commands rather than fuzzy-matching them.

For a verified owner message, the kernel constructs a fresh root `TaskGrant`
with exactly one allowed action (`secret.intake` or `secret.rotate`), zero
other authority lists, a short expiry, and a valid HMAC seal. It constructs an
`ActionRequest` whose only private-adjacent data is the non-secret slot name,
then calls `gate()` with `ActionOrigin::Shell`. The action ids are catalog
members but are not kernel-origin trusted actions. Only `GateDecision::Allow`
can create a pending record.

The pending record is persisted in `kv_state` as JSON and contains:

- slot and intake/rotation mode;
- verified private `chat_id` binding;
- grant id and action request id for audit correlation;
- requested-at and short expiry timestamps.

On the next owner message, the kernel first loads and validates this record
(chat binding and expiry) before treating the text as a secret. If the record
is expired or bound to another chat, the message is **discarded from normal
routing** (never sent to the agent/model), the record is cleared, and the
owner receives metadata-only retry feedback. This fail-closed behavior avoids
turning a delayed or restart-raced protected paste into ordinary agent input.
For a valid record, the text is written to the vault atomically, the pending
record is cleared, and only metadata (`slot`, `mode`, result) is audited and
sent to the owner. The plaintext is never retained in a task grant, artifact,
error, or log.

## Connector broker resolution

`GmailConnector` keeps client id/mailbox/configuration and a shared
`Arc<SecretStore>`, plus the two slot names. Every access-token refresh reads
both slots. Its short-lived access-token cache remains valid until expiry;
rotating the refresh credentials invalidates the cached token so the next call
re-resolves and refreshes with the new value.

`TelegramConnector` keeps a shared vault and token slot alongside the current
`teloxide::Bot` and token. Every poll/send/callback call resolves the slot;
when the token differs, it rebuilds the bot while preserving the configured
API URL used by tests. Thus a long-running kernel adopts a rotated bot token
without process reconstruction.

## Authority and containment

- D-004/D-007: both mode transitions are ordinary `gate()` requests and only
  an explicit allowed action in the signed owner grant can create pending
  state; plaintext capture is a post-authorization kernel effect.
- D-005/D-010: connector credentials and captured values stay kernel-owned;
  the shell receives only ordinary task metadata and bounded result metadata.
  The next message is intercepted before `run_pipeline`, so no shell process
  or model gateway sees it.
- D-006: Telegram verification is an input to composing the owner grant and
  binding the pending record; it is not itself treated as a live authority
  object.
- D-008: command parsing and slot lookup are exact and deterministic.
- D-011: the metadata action request is immutable and gate-mediated; the
  secret value is a separate direct vault capture, never a shell-mutated
  approved payload.
- D-012: audits contain slot/mode/result and action/grant ids only; no secret
  payload is inserted into the audit chain.

## Paired Gmail staging state machine

Gmail needs two correlated credentials (OAuth client secret and refresh
token), so neither is trusted live until the pair is complete and the
connector validates it. The first captured half is written only to a staging
slot (`secret.staged.<slot>`) with correlation metadata
(`secret.stage.<counterpart>`) that carries a finite TTL and a correlation id
binding the two halves. The live counterpart is not written and the staged
value is not activated until the paired credential arrives; at that point the
connector validates the pair against Google. Promotion to both live slots is
atomic: the kernel snapshots the pre-promotion state of both live slots, the
staged credential value, and the staging metadata, then performs the candidate
put, counterpart put, staged-ciphertext delete, metadata delete, and audit
append in order. Any failure after the first mutation rolls back the full
snapshot (both live slots, staged credential value, metadata) and returns a
fatal error; a failed rollback is itself fatal, so a partially-promoted pair
can never be left behind.

## Telegram poll offset namespace

The long-poll loop persists the last processed `update_id` namespaced by the
current bot id (`last_telegram_update_id.<bot_id>`). Before the kernel knows
its bot id (legacy bootstrap), the non-namespaced key is used. Once a
`/secret` token intake stores the bot id, the legacy consumed offset is
migrated into the bot-id-namespaced key exactly once and the legacy key is
cleared. Because the namespace is derived from the current bot id, a same-bot
token rotation keeps the same namespace and therefore preserves the consumed
offset (no redelivery of an already-consumed, possibly secret, update), while
a different-bot rotation yields a distinct namespace that starts fresh.

## Failure modes

Missing root key or malformed ciphertext fails closed. Missing slot values
make connector calls return their existing typed configuration/connector
error; they do not consult a runtime env fallback. The pending record is cleared before validation/write occurs; on any validation, write, or pipeline Err, the pending state is deleted, requiring a fresh `/secret` re-arm. The capture fails closed and never leaves a stale pending intake. Expired or mismatched pending state discards the message and requires a fresh gated command.

## Alternatives rejected

- **Secret in `ActionRequest` payload:** rejected because it crosses the
  shell↔kernel HTTP contract and violates D-014's direct next-message vault
  capture.
- **Kernel-origin auto-allow:** rejected because it widens D-055's trusted
  carve-out and bypasses owner authority. The normal shell-origin gate path
  evaluates a real signed, narrowly scoped owner grant.
- **Runtime env fallback:** rejected because D-025 says D-014 retires the
  env-var bootstrap path. Env values are only first-run seeds.
- **SQLite plaintext or a new crypto scheme:** rejected; the vault mirrors
  `ArtifactStore`'s established AES-256-GCM at-rest pattern and keeps
  plaintext out of shared store rows.
- **In-memory pending state:** rejected because restart would lose the
  owner's explicitly requested mode; persisted state is safe only with the
  chat binding and expiry checks above.
