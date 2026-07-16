# Design: Implement escalation and refusal

## Approach

Three authority-narrowing pieces wired into the real worker-facing chokepoint
(`POST /v1/actions`): a pure surface function that separates worker-safe text
from owner-only escalation; an integrated denial branch that **delivers** the
escalation to the owner control channel and returns only the canonical
deferral to the worker; and dormant `thread_id` fields with a kernel-owned
binding resolver. The no-leak invariant is structural: the owner-only
`EscalationNotice` is never part of the worker HTTP response. Audit is a
separate consequence of delivery, not the routing mechanism itself.

### 1. Pure surface types — schemas/escalation.rs

- `CANONICAL_DEFERRAL: &str = "I need to check on that — I'll get back to you"`.
  ONE constant. Phrasing is learnable presentation (AD-135/AD-151); the
  invariant that this is the *only* counterparty-facing refusal is kernel.
- `EscalationNotice { task_grant_id, denied_action, reason: DenialReason,
  decision: GateDecision, counterparty: Option<String>, thread_id:
  Option<String>, escalated_at, schema_version }` — gate-denial adapter,
  owner-only. Never serialized into the worker HTTP response.
- `EscalationPayload` is a tagged producer-specific enum:
  `GateDenial { action, decision, reason, summary }` or
  `WorkerConfidence { summary }`. Invalid combinations are unrepresentable,
  so worker-runtime confidence escalations do not fabricate gate fields.
- `EscalationEvent { task_grant_id, payload, thread_id, occurred_at,
  schema_version }` is the generic owner-routing envelope.
- `WorkerFacingDeferral` — always `CANONICAL_DEFERRAL`. The only human text a
  worker may relay to a counterparty.
- `surface_denial(grant, action, decision, counterparty, now) ->
  Option<(WorkerFacingDeferral, EscalationNotice)>`: pure function, no I/O.
  Deny/ApprovalRequired → `Some`; Allow/EffectSuppressed → `None`.

The pure denial adapter is converted to `EscalationEvent::GateDenial` before
calling the reusable kernel router. The owner-only event is never put in the
worker response.

The two return values are deliberately separate types so a caller cannot
accidentally stuff the escalation into the worker response. The integration
point (below) is the only place that routes each half.

### 2. Integrated chokepoint — POST /v1/actions denial branch

Today `post_actions` returns the raw `GateDecision` (including
`DenialReason` enum codes) to the worker with no owner delivery and no
counterparty-safe text. After this change the denial branch does:

1. Call `surface_denial(...)`, then convert the owner-only notice to the
   generic tagged `EscalationEvent::GateDenial`.
2. On `Some((deferral, escalation))`:
   - **Route to the owner (AD-133):** call the reusable kernel
     `route_escalation(state, grant, event)`. It resolves the persisted
     task's `bound_chat_id` itself and delivers through the mandatory gated
     `owner.notify` path (`notify_owner_required` → Telegram `send_reply`).
     Destination is deterministic from the task grant, not supplied by a
     producer. Missing HMAC key, gate denial, or connector failure records
     `owner.notify_failed` and returns a structured error; it cannot be
     reported as a successful escalation.
   - **Record as a separate consequence:** only after owner delivery
     succeeds, the router appends audit kind `action.escalated` with the typed
     `GateDecision`, denied action, task grant id, and enum reason code. Audit
     is durable record, not routing.
   - Return `ActionResponseBody` with:
     - `decision: GateDecision` — workers still receive **outcomes** (enum
       reason codes per AD-151), never policy text.
     - `counterparty_deferral: Some(CANONICAL_DEFERRAL)` — the only human
       text the worker may relay.
     - `result: None`
3. On `None` (Allow path): unchanged allow/dispatch flow.

`EscalationNotice` is never a field on `ActionResponseBody`. The worker
response and the owner delivery are separate channels. Audit records both
the gate decision (`action.gated`) and the escalation (`action.escalated`)
as consequences of the denial, not as the delivery itself.

Existing wire consumers that read `decision.outcome` / `decision.reason`
keep working — enum codes are outcomes, not policy text. The new optional
`counterparty_deferral` field is additive.

### 3. Thread_id fields — EventEnvelope + TaskGrant

- Both structs gain `#[serde(default)] pub thread_id: Option<String>`.
  Backward-compatible under `deny_unknown_fields`. Default `None`.
- **MAC-covered when populated, legacy-stable when absent.** `RootAuthority`
  includes `thread_id` in its canonical commitment when `Some`; when `None`,
  the key is omitted so pre-thread grants retain their canonical bytes. The
  shell cannot rewrite a populated binding without resealing.
- No production path populates it.

### 4. Thread↔grant binding resolver — kernel/escalation.rs

- `resolve_grant_for_thread(grants, thread_id) -> Option<&TaskGrant>`:
  - `Some(tid)` → first grant whose `thread_id.as_deref() == Some(tid)`.
  - `None` → first grant whose `thread_id.is_none()` (master thread).
### 5. Mandatory owner delivery from the API layer

The existing `notify_owner_best_effort` helper remains for courtesy pipeline
messages. Security escalations use a separate result-returning
`notify_owner_required` path. It applies the same kernel-origin `gate()` and
owner-channel binding, but propagates missing-key, gate, audit, and connector
failures. `route_escalation` appends `action.escalated` only after this path
returns success.

## Key decisions

- **Wire the real denial branch, not a free-floating helper.** Acceptance
  is exercised through `POST /v1/actions`: a denied action returns the
  canonical deferral, delivers to the owner, and leaves an
  `action.escalated` audit row.
- **Mandatory delivery is distinct from courtesy notification.** AD-133
  escalation routing cannot swallow missing-key, gate, or connector failures.
  `notify_owner_required` records `owner.notify_failed` and returns an error;
  `action.escalated` is appended only after connector success.
- **Escalation and worker response are separate channels.** Returning a
  combined object with both text and notice would re-expose the owner-only
  reason to the worker.
- **Enum reason codes stay on the worker response.** AD-151: workers
  receive outcomes, not policy text. Free-form policy prose never crosses.
- **`ApprovalRequired` also escalates.** A worker waiting on approval must
  not invent a policy explanation for a counterparty either.
- **`thread_id` is MAC-covered but omitted when `None`.** The optional field
  participates in the root commitment when populated, while the canonical
  bytes preserve the pre-thread form for legacy grants. A `None`→`Some`
  mutation still fails verification without resealing.

## Alternatives considered

- **Return raw `DenialReason` with no deferral/escalation.** Status quo —
  rejected. No owner surface, no counterparty-safe text.
- **Return `EscalationNotice` inside the worker response.** Rejected —
  re-exposes owner-only reason on the worker channel.
- **Audit-only "routing".** Rejected — AD-133 is deterministic *delivery*
  to the owner, not just a durable log row. Audit is a separate consequence
  (same pattern as `owner.notified` vs the Telegram send).
- **Strip `DenialReason` from the worker response entirely.** Rejected —
  AD-151 keeps enum reason codes as worker outcomes; existing wire tests
  and worker control flow depend on them. The leak is policy *text*, not
  the enum.
- **Configurable refusal strings per denial reason.** Rejected — ONE
  canonical policy-free refusal (AD-151).
- **MAC-cover `thread_id` only when active.** Rejected — AD-148 makes the
  binding kernel-owned; integrity must hold before a channel activates. The
  field is dormant in use, not unprotected in storage.

## Authority sensitivity

1. **D-004 deny-by-default** — the chokepoint does not change what gate
   decides; it only rewrites how a denial is *surfaced*.
2. **D-006 identity-is-not-authority** — optional `counterparty` on
   `EscalationNotice` is a display hint, never authority.
3. **D-007 grant-is-the-only-live-authority** — `thread_id` is routing, not
   authority. Binding selects *which* grant, never *what* it authorizes.
4. **D-008 deterministic routing** — pure deterministic functions; no agent
   choice, no LLM, no personality. Owner destination is the grant's bound
   chat.

## What does NOT move

- Gate decision semantics. `DenialReason` enum variants. Action catalog.
- Grant chain MAC construction. Audit chain structure (new *kind* only).
- Worker runtime / shell spawn.
- Telegram private-chat requirement. No group-visible authority.
- Full AD-138 failure-surfacing (dead-letter, metrics) — this change records
  truthful `owner.notify_failed` outcomes and propagates delivery failures,
  but does not add the full dead-letter queue or metrics subsystem.
