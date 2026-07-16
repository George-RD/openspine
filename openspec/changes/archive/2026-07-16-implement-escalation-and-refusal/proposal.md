# Proposal: Implement escalation and refusal

## Summary

Introduce the deterministic kernel escalation routing machinery and the
policy-free refusal invariant so that a gate denial facing a counterparty
surfaces **only** one canonical deferral plus a deterministic escalation event
to the owner — never policy text. Wire this into the real worker-facing
chokepoint (`POST /v1/actions`): the denial branch emits an `action.escalated`
audit event (owner surface) and returns the canonical deferral on the worker
response (counterparty-safe text). Add optional `thread_id` fields on
`EventEnvelope` and `TaskGrant` with a kernel-owned thread↔grant binding
resolver that stays **dormant** until a thread-capable channel ships.

## Dependencies

- `implement-identity-store-and-principal` (archived). Worker escalation events
  from `implement-worker-runtime` are the main producer of escalations, but the
  routing machinery itself does not depend on it — this change builds and wires
  the machinery now.

## Problem/Context

Today `gate()` returns a `GateDecision` (including `DenialReason` enum codes)
and `POST /v1/actions` hands that decision straight back to the shell/worker
(`api/actions.rs`) with no owner-visible escalation and no counterparty-safe
text. A worker can invent a human-readable policy explanation for a
counterparty ("I'm not allowed to send emails because…"). AD-151 settles the
invariant: "Gate denials stay enum reason codes; workers receive outcomes, not
policy text. The spec ships ONE canonical policy-free refusal ('I need to check
on that — I'll get back to you') plus deterministic escalation to the owner."
"I'm not allowed to discuss X" is itself a disclosure.

AD-133 settles that escalation routing is deterministic kernel machinery (route
by task; workers talk to the owner only when escalating), and AD-148 settles
that `EventEnvelope` and `TaskGrant` gain optional `thread_id` with kernel-owned
thread↔grant binding, dormant until a thread-capable channel ships.

None of this machinery exists yet, and no free-floating helper would satisfy the
acceptance path without integrating the real denial branch.

## Proposed Solution

1. **Canonical deferral + surface types** (`openspine-schemas/src/escalation.rs`):
   - `CANONICAL_DEFERRAL` — the one policy-free refusal string.
   - `EscalationNotice` — owner-only gate-denial record (real
     `DenialReason`, typed `GateDecision`, denied action, task grant id).
   - `EscalationPayload` — tagged producer-specific payload: gate denial or
     worker confidence; invalid combinations are unrepresentable.
   - `EscalationEvent` — generic owner-routing envelope shared by all future
     escalation producers.
   - `WorkerFacingDeferral` — carries only `CANONICAL_DEFERRAL`.
   - `surface_denial()` — pure function returning
     `Option<(WorkerFacingDeferral, EscalationNotice)>`. Deny/ApprovalRequired
     → `Some`; Allow/EffectSuppressed → `None`.

2. **Integrated chokepoint** (`POST /v1/actions` denial branch):
   - On a denial of a **counterparty-facing** action:
     - **Route to the owner** via mandatory gated `owner.notify` / Telegram
       send to the task's bound owner chat (deterministic from the grant).
       Missing-key, gate, and connector failures record `owner.notify_failed`
       and return structured errors; audit persistence failures propagate
       without guaranteeing the failure record lands. They never become a
       false successful escalation.
     - **Audit separately** as `action.escalated` only after owner delivery
       succeeds (durable record, not the routing mechanism).
     - Return `ActionResponseBody { decision, counterparty_deferral:
       Some(CANONICAL_DEFERRAL), result: None }`.
   - Workers still receive enum reason codes as outcomes (AD-151). They never
     receive free-form policy text or the `EscalationNotice`.
   - Denials of non-counterparty actions retain the ordinary enum-only
     response.
   - On Allow: unchanged.

3. **Schema additions**: `#[serde(default)] thread_id: Option<String>` on
   `EventEnvelope` and `TaskGrant`. A populated TaskGrant binding is included
   in the root MAC commitment; when `None`, the key is omitted to preserve
   pre-thread canonical bytes.

## Acceptance Criteria

- A denied `POST /v1/actions` returns `counterparty_deferral` equal to the
  canonical deferral, delivers an owner-facing escalation message on the
  owner control channel, and leaves an `action.escalated` audit event; no
  free-form policy text appears in the worker-facing response body.
- A counterparty-facing denial whose owner delivery fails returns a non-2xx
  structured error, records `owner.notify_failed`, and does not record
  `action.escalated`.
- Enum reason codes remain on the worker `decision` (outcomes, not policy text).
- `EscalationNotice` is never a field of the worker HTTP response.
- `thread_id` fields are present, default to `None`, and are unused by
  production paths.
- `resolve_grant_for_thread` resolves deterministically but is dormant.

## Out of Scope

- Worker runtime / worker escalation event production
  (`implement-worker-runtime`).
- Full AD-138 failure-surfacing (dead-letter queue, metrics counters); this
  change records truthful notification failures but does not add the complete
  dead-letter/metrics subsystem.
- Thread-capable channel implementation and activation (the dormant,
  already MAC-authenticated binding is not populated or consumed yet).
- Presentation phrasing (learnable overlay, AD-135).
