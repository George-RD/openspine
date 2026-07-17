# Proposal: Implement secret intake

## Summary

Replace the env-var-only bootstrap for connector credentials (Telegram bot
token, Gmail OAuth client secret/refresh token) with a kernel-side encrypted
secret vault plus an owner-gated intake/rotation flow: the owner triggers
intake with a `/secret intake|rotate <slot>` command, the mode-switch is
authorized through `gate()` with a narrowly-scoped signed owner grant, and
the secret *value* itself is captured directly from the owner's next verified
Telegram message — never through an action payload, never through the
shell↔kernel HTTP contract, never through the agent/model. Connectors
(`GmailConnector`, `TelegramConnector`) resolve their credentials from the
vault at call time, so introducing or rotating a secret takes effect on the
next connector call with no kernel restart.

## What Changes

- New `crates/openspine-kernel/src/secret_store.rs`: a file-based,
  AES-256-GCM-encrypted-at-rest secret vault under `data/credentials/`,
  keyed by the existing `OPENSPINE_ARTIFACT_KEY` (same key material pattern
  as `ArtifactStore`, per D-025's "OAuth tokens are encrypted at rest under
  `data/credentials/`"). Unlike `ArtifactStore` (content-addressed,
  immutable), `SecretStore` is name-addressed and mutable — `put`
  overwrites, which is exactly what rotation needs. `seed_if_absent` lets
  startup seed bootstrap env values into the vault without ever clobbering a
  value the owner already rotated in.
- New `crates/openspine-kernel/src/secret_intake.rs`: the `/secret
  intake|rotate <slot>` command parser (mirrors `parse_draft_command`/
  `parse_bind_command`), the gate()-mediated mode-switch (constructs a
  narrowly-scoped, HMAC-sealed owner `TaskGrant` whose *only*
  `allowed_actions` entry is `secret.intake` or `secret.rotate`, submits an
  `ActionRequest` carrying only the slot name as metadata through `gate()`
  with `ActionOrigin::Shell`), and the pending-intake record (slot, mode,
  `chat_id`, grant/request ids, a short expiry) persisted in `kv_state`.
- Gmail OAuth credentials use a paired staging state machine: the first half
  is stored only in `secret.staged.<slot>` with correlation metadata
  (`secret.stage.<counterpart>`) and a five-minute TTL; the second half must
  arrive with the same correlation before the connector validates and
  atomically promotes both live slots. Any failure after the first promotion
  mutation restores both live slots, the staged credential value, and the
  metadata, and a rollback failure is fatal.
- Telegram poll offsets are namespaced by the validated current bot id. When
  the id is first learned, a legacy consumed offset is migrated exactly once
  into that namespace and the legacy key cleared; later different-bot
  rotations start fresh, while same-bot token rotation preserves the consumed
  offset and cannot redeliver an already-consumed update.
- `secret.intake` and `secret.rotate` join the action catalog as ordinary
  entries — **not** added to the kernel-origin trusted set (that set stays
  exactly `{owner.notify}`; widening it would convert an owner-authorized
  operation into a kernel auto-allow, contradicting D-055.3's "enumerated
  trusted-origin set").
- `handle_owner_update` (`pipeline/mod.rs`) gains a pre-pipeline branch,
  checked before lane selection: if a pending intake record exists, is bound
  to this `chat_id`, and has not expired, the incoming message *text* is the
  secret — it is written straight to `SecretStore`, the pending record is
  cleared, and the owner gets a metadata-only confirmation. The message
  never reaches `run_pipeline`, never spawns a shell, never enters a model
- A stale/mismatched/expired pending record is dropped, the message is
  discarded, and the owner receives retry metadata; it fails closed rather
  than falling through to normal routing.
- `GmailConnector.access_token()` resolves `client_secret`/`refresh_token`
  from the vault on every refresh (not from strings cached at construction).
- `TelegramConnector` resolves its bot token from the vault on every
  `poll_once`/`send_reply`/etc. call, rebuilding the underlying `teloxide::
  Bot` only when the resolved token differs from the one currently in use.
- `main.rs` opens the `SecretStore`, seeds it (if-absent) from the existing
  bootstrap env vars (`OPENSPINE_TELEGRAM_BOT_TOKEN`,
  `gmail_client_secret_env`/`gmail_refresh_token_env`), and passes it to the
  connectors and `AppState`. `OPENSPINE_ARTIFACT_KEY` remains the one
  unavoidable env-sourced secret — it is the root key that decrypts the
  vault itself, so it cannot live inside the thing it unlocks. The model
  gateway's provider API key is out of scope (see Non-goals) and stays
  env-sourced.

## Why

D-014: "Setup secrets must be captured by a vault/secret-intake flow, not by
ordinary agent chat... Agent sees only success/failure metadata, not the
secret... No model call is made with the secret." D-025 documents today's
env-var bootstrap as an *explicit, temporary* deferral and states plainly:
"the secret-intake flow (D-014) lands, retiring the env-var bootstrap path."
This change is that landing, scoped to the two connectors D-025 names
(Telegram bot token, Gmail OAuth secrets).

## Affected layer

OpenSpine core (`openspine-kernel`: new `secret_store`/`secret_intake`
modules, `action_catalog`, `gmail`, `telegram`, `pipeline/mod.rs`, `main.rs`;
`openspine-schemas` is modified (`TargetRefKind::SecretSlot` added); `openspine-gate`
is consumed, not modified — the existing `gate()`/`TaskGrant`/`ActionCatalog` API is reused as-is).

## Authority sensitivity

**HIGH — new gate()-mediated action, new credential storage, and a
pre-pipeline message-capture branch.** This change:

- Adds two new catalog actions (`secret.intake`, `secret.rotate`) mediated
  by the normal owner-grant path (D-004: every effectful action goes
  through `gate()`).
- Does **not** widen the kernel-origin trusted set (D-055.3) — the
  mode-switch grant is a real, narrowly-scoped, HMAC-sealed owner grant
  evaluated by `gate()`'s ordinary `effectively_allows` path with
  `ActionOrigin::Shell`, not an auto-allow.
- Adds a pre-pipeline branch that can consume an owner message without
  ever creating a task grant or spawning a shell — bound to `chat_id` and
  a short expiry (D-006: identity is not authority by itself; the *chat_id
  binding + expiry*, not merely "a message arrived from Telegram," is what
  authorizes capture) so a stale record can never silently capture an
  unrelated future message as a credential.
- Removes runtime connector resolution's dependency on env vars after
  bootstrap for the two named connectors, replacing it with an
  encrypted-at-rest vault. Legacy env values remain in the kernel process
  environment until the process exits; shell children still receive a
  cleared environment (D-005).
- The secret value never appears in an `ActionRequest`, never crosses the
  documented kernel↔shell HTTP contract, and is never passed to the agent
  or a model provider (D-014's core requirement, D-010's shell/model
  isolation).

The change is strictly narrowing/hardening: no new effect is exposed to the
shell or the agent; the owner's ability to introduce/rotate connector
credentials is the only new capability, and it requires the same
Telegram-owner-verification precondition every other owner command
(`/draft`, `/bind`) already requires, PLUS gate() mediation on top.

## Goals

- A secret can be introduced and rotated without a kernel restart.
- The very next connector call after rotation uses the new value (verified
  for both Gmail and Telegram).
- The secret plaintext never crosses the kernel↔shell HTTP contract, never
  appears in an audit row, log line, or error message.
- Only the verified owner can enter intake/rotation mode, and only through
  the normal `gate()`-mediated owner-authority path — no kernel-origin
  auto-allow carve-out.

## Non-goals

- The model gateway's provider API key remains env-sourced in this change.
  This is the explicit scope boundary ratified by D-065; the
  foundation-amendment lane is the follow-up owner of this migration, not the
  archived model-gateway change.
- `OPENSPINE_ARTIFACT_KEY` remains env-sourced. It is the root key that
  decrypts both `ArtifactStore` and the new `SecretStore`; it cannot itself
  live inside the vault it unlocks. Rotating the root key is out of scope.
- No interactive OAuth consent flow is added. The owner still completes
  Google's consent screen out of band (unchanged from today) and pastes the
  resulting refresh token through the intake flow instead of an env var.
- No UI/keyboard affordance beyond the existing plain-text Telegram command
  surface (`/secret intake <slot>` / `/secret rotate <slot>`); no
  cancel-in-flight command (a wrong paste is fixed by intaking again, which
  overwrites).
- No multi-owner / multi-tenant secret scoping — there is exactly one owner
  principal today (D-006/D-007's existing v1 constraint), so slots are not
  namespaced per-principal.

## Dependencies

None. The change stands alone; it requires no other in-flight change.

## Problem/Context

Today `crates/openspine-kernel/src/config.rs` reads the Telegram bot token,
the Gmail OAuth client secret, and the Gmail OAuth refresh token from
environment variables at kernel startup (`telegram_bot_token`,
`gmail_client_secret`, `gmail_refresh_token`). `GmailConnector` caches the
client secret and refresh token as owned `String`s at construction
(`main.rs:151-163`); `TelegramConnector` bakes the bot token into a
`teloxide::Bot` at construction (`telegram.rs:282-286`). Rotating either
credential today requires editing the env var and restarting the kernel
process — there is no in-process path to introduce or rotate a secret, and
(per D-014's rationale) no flow exists to *capture* a secret from the owner
without exposing it to ordinary agent chat (and therefore to logs, model
providers, or memory). D-025 explicitly names this the deferred state:
"Env-var bootstrap secrets are an explicitly documented deferral, not a
final answer — D-014's secret-intake flow remains a future change."

## Proposed Solution

See design.md for the full authority analysis. In outline: a file-based
encrypted vault (`SecretStore`, mirroring `ArtifactStore`'s AES-256-GCM
pattern) stores connector secrets by name; a gate()-mediated,
narrowly-scoped owner grant authorizes entering intake/rotation mode for one
named slot; the owner's next verified Telegram message is captured directly
into the vault, bypassing the agent/model entirely (D-014); connectors
re-resolve their credentials from the vault at call time instead of caching
them at construction, so rotation takes effect on the next call with no
restart.

## Acceptance Criteria

- `openspec validate implement-secret-intake --strict` is green.
- A secret can be introduced (`/secret intake <slot>` + next message) and
  later rotated (`/secret rotate <slot>` + next message) without a kernel
  restart; a subsequent `GmailConnector`/`TelegramConnector` call
  demonstrably uses the newly rotated value (same connector instance, no
  reconstruction).
- A dedicated test proves the secret plaintext never appears in any
  kernel↔shell HTTP request/response body captured across an
  intake→rotate→connector-call sequence, nor in the owner-facing
  confirmation message, nor in any audit row.
- `secret.intake`/`secret.rotate` are denied for any request that is not a
  verified-owner-triggered, `gate()`-allowed, narrowly-scoped grant; neither
  action is added to the kernel-origin trusted set.
- A pending intake/rotation record bound to a stale `chat_id` or past its
  expiry is never used to capture a message as a secret.

## Out of Scope

Provider API key migration to the vault, OAuth interactive consent flow,
multi-owner secret scoping, and an in-flight cancel command are all out of
scope (see Non-goals above) and are not partially implemented here.

## Decision-log check

This change does not reverse or weaken any accepted decision. It is the
D-014-anticipated landing that D-025 explicitly names ("the secret-intake
flow (D-014) lands, retiring the env-var bootstrap path") — scoped to the
two connectors D-025's own bootstrap-secret list covers that are
call-time-resolvable (bot token, Gmail OAuth secrets); D-025's own
"provider API keys" item and the artifact-key root secret remain
env-sourced, which is a narrower, more precisely documented version of the
same deferral D-025 already states, not a reversal. D-064 through D-067 ratify the connector-vault migration boundary, provider-key ownership, paired Gmail staging, and Telegram bot-identity cursor semantics implemented by this change.
