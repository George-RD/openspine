# escalation-and-refusal Specification

## Purpose
TBD - created by archiving change implement-escalation-and-refusal. Update Purpose after archive.
## Requirements
### Requirement: Counterparty-facing gate denials at the worker action chokepoint MUST surface only the canonical deferral plus an owner-routed escalation

When `POST /v1/actions` receives a gate denial or approval-required outcome
for a **counterparty-facing** action, the kernel MUST:

1. Deterministically route an owner-facing escalation to the owner control
   channel bound to the denied task (gated `owner.notify` / Telegram send to
   the grant's `bound_chat_id`), AND
2. Append a durable audit event of kind `action.escalated` as a separate
   consequence (recording is not the routing mechanism), carrying the denied
   action, the task grant id, and the full gate decision, AND
3. Return a worker response whose only human-facing text field is the ONE
   canonical policy-free deferral ("I need to check on that — I'll get back to
   you").

Denials of non-counterparty-facing actions retain their typed enum outcome
without escalation or deferral. Workers MUST receive outcomes (typed
`GateDecision` enum reason codes) and the canonical deferral when applicable.
Workers MUST NOT receive free-form policy text or an `EscalationNotice` object.
The chokepoint MUST NOT construct a human-readable policy explanation from the
denial reason. "I'm not allowed to discuss X" is itself a disclosure and MUST
NOT be produced.

#### Scenario: Counterparty denial returns canonical deferral, routes to owner, and audits

- **GIVEN** a task grant that explicitly denies the counterparty-facing
  `email.send` action
- **WHEN** the worker calls `POST /v1/actions` for `email.send`
- **THEN** the HTTP response MUST include `counterparty_deferral` equal to
  the canonical deferral constant
- **AND** the response `decision.outcome` MUST be `deny`
- **AND** the response body MUST NOT contain free-form policy prose explaining
  the denial
- **AND** an owner-facing notification MUST be delivered on the task's bound
  owner chat
- **AND** an audit event of kind `action.escalated` MUST be recorded for the
  task grant

#### Scenario: Counterparty ApprovalRequired also returns deferral, routes to owner, and audits

- **GIVEN** a task grant that requires approval for the counterparty-facing
  `email.send` action
- **WHEN** the worker calls `POST /v1/actions` for `email.send` without an
  approval
- **THEN** the HTTP response MUST include `counterparty_deferral` equal to
  the canonical deferral constant
- **AND** an owner-facing notification MUST be delivered on the task's bound
  owner chat
- **AND** an audit event of kind `action.escalated` MUST be recorded

#### Scenario: Failed owner delivery is surfaced without false escalation success

- **GIVEN** a counterparty-facing denial whose owner Telegram connector returns
  an error
- **WHEN** the worker calls `POST /v1/actions`
- **THEN** the HTTP response MUST be non-2xx
- **AND** an audit event of kind `owner.notify_failed` MUST be recorded
- **AND** no `action.escalated` audit event MUST be recorded

#### Scenario: Allowed action does not escalate or defer

- **GIVEN** a task grant that allows `openspine.status.read`
- **WHEN** the worker calls `POST /v1/actions` for `openspine.status.read`
- **THEN** the HTTP response MUST NOT include `counterparty_deferral`
- **AND** no audit event of kind `action.escalated` MUST be recorded for that
  request

### Requirement: No policy or rule text MUST cross the worker-facing chokepoint as human-facing content

The only human-facing text the worker-facing chokepoint MAY attach to a denial
response is the canonical deferral constant. It MUST NOT contain free-form
explanations of why the action was denied, policy rule identifiers, or phrases
of the form "I'm not allowed to…". Enum reason codes on the typed
`GateDecision` are machine outcomes and MAY remain on the response.

#### Scenario: Counterparty deferral is exactly the canonical constant

- **GIVEN** a denied `POST /v1/actions` request
- **WHEN** the response is inspected
- **THEN** `counterparty_deferral` MUST equal exactly
  `"I need to check on that — I'll get back to you"`

#### Scenario: Pure surface function never puts reason text into the deferral

- **GIVEN** each variant of `DenialReason`
- **WHEN** `surface_denial` is invoked for a Deny carrying that reason
- **THEN** the worker-facing deferral text MUST equal the canonical constant
- **AND** the text MUST NOT contain the reason's snake_case name

### Requirement: Escalation routing MUST be deterministic kernel machinery

Escalation routing MUST be a pure deterministic function of the task grant and
the denial outcome. A worker talks to the owner only when escalating. The
escalation audit event MUST carry the task grant id so the owner can identify
which task escalated. Routing MUST NOT depend on agent personality, LLM choice,
or presentation preferences.

#### Scenario: Escalation audit binds to the task grant

- **GIVEN** a task grant with a known id
- **WHEN** a denial is surfaced through `POST /v1/actions`
- **THEN** the `action.escalated` audit event's `task_grant_id` MUST equal the
  grant's id

### Requirement: Thread↔grant binding MUST be kernel-owned and dormant until a thread-capable channel ships

`EventEnvelope` and `TaskGrant` MUST carry an optional `thread_id` field.
Binding is kernel-owned: a reply in thread T resolves to the grant bound to T;
no binding resolves to the master thread (grant with `thread_id = None`). The
shell MUST NOT create or switch threads. The binding resolver MUST exist and
be deterministic, but MUST remain dormant (no production call site) until a
thread-capable channel ships. Telegram topics are group-only and owner control
is private-chat-only, so the binding does not activate on Telegram.

#### Scenario: Resolve by thread id returns the bound grant

- **GIVEN** two grants, one with `thread_id = Some("t1")` and one with
  `thread_id = None`
- **WHEN** `resolve_grant_for_thread` is called with `Some("t1")`
- **THEN** it MUST return the grant bound to `"t1"`

#### Scenario: No thread id resolves to the master thread

- **GIVEN** two grants, one with `thread_id = Some("t1")` and one with
  `thread_id = None`
- **WHEN** `resolve_grant_for_thread` is called with `None`
- **THEN** it MUST return the grant with `thread_id = None`

#### Scenario: thread_id fields default to None

- **GIVEN** a freshly constructed `EventEnvelope` or `TaskGrant` without an
  explicit `thread_id`
- **WHEN** the value is inspected
- **THEN** `thread_id` MUST be `None`

