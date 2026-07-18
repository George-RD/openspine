# Spec: Core runtime schemas

## Purpose

Define explicit, versioned schemas for every core OpenSpine runtime object — event envelope, identity, route, task grant, action request, gate decision, approval, artifact, audit — before runtime implementation relies on them, so identity, routing, and authority stay structurally separated from the start.
## Requirements
### Requirement: OpenSpine core runtime objects MUST have explicit schemas

OpenSpine core runtime objects MUST have explicit schemas before runtime implementation relies on them.

Core runtime objects MUST include event envelope, identity resolution, route artifact, agent manifest, workflow manifest, capability pack, authority composition input/output, task grant, action request, gate decision, approval record, selection token, model request, audit event, artifact reference, and principal.

#### Scenario: Runtime object is added

Given an implementation introduces a new runtime object
When that object participates in routing, authority, action mediation, model access, memory, connector access, audit, or approval
Then the object MUST have an explicit schema
And the schema MUST be versioned.

### Requirement: Event envelopes MUST include source authenticity fields

Every event envelope MUST include source, connector, account role, event type, received timestamp, verified source status, verification method, replay protection status, actor hints, target refs, data classification, lane, and trust context.

#### Scenario: Telegram owner event is normalized

Given a Telegram owner message is received
When the event envelope is created
Then it MUST include source, connector, event type, verified source, verification method, lane, and trust context.

### Requirement: Identity schemas MUST NOT grant runtime authority

Identity and Principal records MUST store entity knowledge only.

Identity and Principal records MUST NOT directly attach live capability packs, active routes, live tool access, or task grants.

Identity resolution MUST return an optional principal_id that is Some only for the owner in v1.

#### Scenario: Known owner identity exists

Given an identity record represents the owner
When a Telegram message is received
Then the identity record MAY contribute relationship and confidence information
But it MUST NOT grant authority by itself.

### Requirement: Route schemas MUST be declarative artifacts

Routes MUST be represented as declarative, versioned artifacts.

Routes MUST map event/context conditions to candidate agent, workflow, and capability pack references.

Routes MUST NOT directly grant final runtime authority.

#### Scenario: Owner Telegram route exists

Given a route matches `telegram.owner.message`
When route resolution succeeds
Then the route MAY select `main_assistant_agent`, `owner_control_conversation`, and `owner_control_basic_pack`
But final authority MUST still be materialized through a task grant.

### Requirement: Route resolution schemas MUST represent ambiguity

Route resolution MUST represent success, denial, and ambiguity.

Ambiguous route matches MUST fall back to low-authority triage or review.

#### Scenario: Two routes match without deterministic winner

Given two active routes match an event
And no deterministic priority or specificity rule selects a winner
When route resolution runs
Then the result MUST be ambiguous
And it MUST NOT grant widened authority.

### Requirement: Task grants MUST be explicit live authority objects

Task grants MUST be short-lived, purpose-bound, route-bound, agent-bound,
workflow-bound, and target-bound where applicable. Running agents and workflows
MUST receive a task grant rather than broad permissions.

A task grant MUST carry an authenticated Macaroons-simple `chain` of ordered
`GrantChainStep` records. Each step contains its `grant_id`, optional
`parent_grant_id`, `mode`, selection-token bindings, and only the caveats added
at that hop. The chain tip `caveat_mac` authenticates the immutable root
authority and every ordered hop. Roots have one empty-caveat step; children
append one step derived from the parent's terminal MAC. `mode` is `live` or
`shadow` (default `live`). Caveat kinds include action allowlists, AD-036 bound
parameters, earlier expiry, model tier, and output-channel allowlists.

The chain is the attenuation proof; a child MUST NOT expand effective actions,
selection tokens, output channels, or execution mode relative to prior hops.
A sub-grant is still a task grant — the only live authority object presented to
a worker (D-007); its parent is lineage only.

#### Scenario: Root grant defaults

Given a newly composed root task grant
When it is inspected
Then its chain has one root step with no parent and no added caveats
And its `caveat_mac` is valid under the kernel-owned verification key.

#### Scenario: Sub-grant is the sole presented authority

Given a parent grant and an attenuated child with a chained delegation step
When a worker starts
Then it receives the child task grant only
And the parent is not a second live authority source.

#### Scenario: Bound parameters are caveats

Given an effectful call has an identity- or scope-bearing parameter
When authority is materialised
Then the binding is represented by a `bound_parameter` caveat
And conflicting values for the same name are rejected as caveat widening.

#### Scenario: Email reply drafter starts

Given a selected-thread email drafting workflow starts
When the workflow is invoked
Then the email reply drafter MUST receive a task grant
And the task grant MUST include allowed, denied, and approval-required actions.

### Requirement: Action requests and gate decisions MUST be typed

Every effectful action MUST be represented as a typed action request.

Every gate result MUST be represented as a typed gate decision.

#### Scenario: Agent requests email thread read

Given an agent requests to read an email thread
When the request reaches gate()
Then gate() MUST evaluate a typed action request against the task grant
And return an allow, deny, or approval-required decision.

### Requirement: Approval records MUST bind reviewed payloads and targets

Approval records MUST bind the exact reviewed payload digest and target digest.

Any mutation to body, recipient, target, thread, or payload MUST invalidate approval.

#### Scenario: Draft body changes after approval

Given the user approved a draft body
When the body changes before execution
Then the prior approval MUST NOT authorize the changed payload.

### Requirement: Audit schemas MUST reference private payloads by encrypted or hash refs

Audit events MUST store metadata directly.

Private payloads MUST be stored as encrypted artifact refs, hash refs, or
equivalent protected references rather than raw plaintext audit text.

Every audit event schema MUST include a unique event ID, an `aggregate_id`,
and a per-aggregate `aggregate_seq` so consumers can deduplicate and order by
aggregate stream without a parallel event store.

#### Scenario: Model request includes private email content

Given a model request includes private email context
When audit is written
Then raw private content MUST NOT be written directly into the audit event
And the audit event MUST reference protected artifact refs and hashes.

#### Scenario: Audit event carries aggregate stream coordinates

Given an audit event is appended to the ledger
When the persisted audit event is inspected
Then it MUST include a unique event ID
And it MUST include an `aggregate_id` and a positive `aggregate_seq`.

### Requirement: Event bus subscription types MUST be explicit schemas

OpenSpine MUST define explicit, versioned schema types for event-bus
subscription filters and consumer checkpoints.

A subscription filter MUST be able to constrain audit kind and/or
`aggregate_id`.

A consumer checkpoint MUST record the last successfully handled global ledger
sequence for a named consumer.

#### Scenario: Filter and checkpoint types exist

Given a consumer is configured against the event bus
When its filter and checkpoint are serialized
Then both MUST round-trip through the schema types
And unknown fields MUST be rejected.

### Requirement: EventEnvelope MUST carry an optional dormant thread_id

The normalized event envelope (PRD §4.1) MUST include an optional
`thread_id: Option<String>` field. The field defaults to `None` and is
backward-compatible under `deny_unknown_fields` via `serde(default)`. The
field is dormant until a thread-capable channel ships (AD-148); no production
path populates it in v1.

#### Scenario: EventEnvelope without thread_id deserializes as None

- **GIVEN** a serialized EventEnvelope that does not include a `thread_id` key
- **WHEN** it is deserialized
- **THEN** `thread_id` MUST be `None`

#### Scenario: EventEnvelope with thread_id round-trips

- **GIVEN** an EventEnvelope with `thread_id = Some("topic-42")`
- **WHEN** it is serialized and deserialized
- **THEN** `thread_id` MUST equal `Some("topic-42")`

### Requirement: TaskGrant MUST carry an optional dormant thread_id

The task grant (D-007) MUST include an optional `thread_id: Option<String>`
field. The field defaults to `None` and is backward-compatible under
`deny_unknown_fields` via `serde(default)`. The field is a kernel-owned
routing/binding field, not a source of authority, but it MUST be included in
the `RootAuthority` MAC commitment even while dormant so the shell cannot
rewrite the binding. When a thread-capable channel ships, activation begins
populating/using the already authenticated field.

#### Scenario: TaskGrant without thread_id deserializes as None

- **GIVEN** a serialized TaskGrant that does not include a `thread_id` key
- **WHEN** it is deserialized
- **THEN** `thread_id` MUST be `None`

#### Scenario: TaskGrant with thread_id round-trips

- **GIVEN** a TaskGrant with `thread_id = Some("topic-42")`
- **WHEN** it is serialized and deserialized
- **THEN** `thread_id` MUST equal `Some("topic-42")`

#### Scenario: Mutating thread_id invalidates the grant MAC

- **GIVEN** a sealed TaskGrant with `thread_id = None`
- **WHEN** its `thread_id` is changed to `Some("topic-42")` without resealing
- **THEN** MAC verification MUST fail

### Requirement: Worker sub-grants MUST be offline-verifiable caveat-chain attenuations
A commissioned worker's grant MUST be a caveat-chain child of the commissioning
master's own grant, where the child's MAC extends the parent's sealed tip. The child
MUST verify offline against only the HMAC key and its embedded chain, with no parent
or root database lookup. A child MUST never widen the parent's authority.

#### Scenario: Offline verification of a multi-level chain (`offline_chain_verify_multi_level`)
- **GIVEN** a master grant and a worker sub-grant minted from it, and a second worker
  sub-grant minted from that worker
- **WHEN** the chain is verified using only the HMAC key and the child's embedded chain
- **THEN** verification succeeds without any store or network access

#### Scenario: Widening action is rejected (`child_cannot_widen_parent_action`)
- **GIVEN** a master grant whose effective allowed actions do not include `email.send_draft`
- **WHEN** a worker sub-grant is minted requesting `email.send_draft`
- **THEN** minting is rejected
- **AND** no child grant is produced

#### Scenario: Widening expiry is rejected (`child_cannot_widen_parent_expiry`)
- **GIVEN** a master grant with an effective expiry
- **WHEN** a worker sub-grant is minted with an expiry later than the master's
- **THEN** minting is rejected

### Requirement: Master MUST commission workers and relay results as bus events
A master agent MAY commission a worker by minting a narrowed sub-grant of its own
grant and packing the worker's briefcase. The worker's result MUST return as a
`worker.result` bus event on the worker grant's aggregate; the master MUST consume it
through the ordinary event bus and relay through its own separately-gated reply path.

#### Scenario: Worker commissioned and result consumed (`result_is_consumed_bus_event`)
- **GIVEN** a master grant authorized for `worker.commission`
- **WHEN** the master commissions a worker and the worker reports a result
- **THEN** a single `worker.result` bus event is recorded on the worker grant's
  aggregate carrying the structured payload
- **AND** the master consumes it through the normal event-bus path

#### Scenario: Parent allows action but worker denies it (`worker_denied_outside_narrowed_allowlist`)
- **GIVEN** a parent grant allows `openspine.status.read` and the worker grant narrows that action away
- **WHEN** the parent and worker requests for `openspine.status.read` are evaluated at the gate
- **THEN** the parent request is allowed
- **AND** the worker request is denied
#### Scenario: Worker report action remains allowed (`worker_allowed_exact_report_action`)
- **GIVEN** a worker grant is narrowed to the exact `worker.report_result` action
- **WHEN** the worker report request is evaluated at the gate
- **THEN** the gate decision is `Allow`

#### Scenario: Classified empty output channel is denied (`empty_declared_output_channels_fail_closed`)
- **GIVEN** the action catalog classifies `test.output` as an output action with an empty channel vector
- **WHEN** a grant allowing `test.output` is evaluated at the gate
- **THEN** the gate denies with `OutputChannelNotGranted`

### Requirement: Worker grants MUST have no effective output channel (reply chokepoint)
A commissioned worker grant MUST carry an empty `OutputChannelAllowlist` caveat so its
effective output channels are provably empty regardless of the root's channel list. A
worker MUST NOT egress directly; its only outbound path is `worker.report_result`.

#### Scenario: Worker cannot egress directly (`classified_empty_output_channel_denial`)
- **GIVEN** a root grant that carries an output channel
- **WHEN** a worker sub-grant is minted from it
- **THEN** the worker's effective output channels are empty
- **AND** `effectively_allows_output_channel` returns false for every root channel

### Requirement: Worker result recording MUST be receipt-keyed and fail-closed (D-083)
Recording a worker result MUST mark the dispatch terminal and MUST reject a second
result for an already-terminal dispatch, never replaying it. The terminal flip and the
receipt check MUST share no TOCTOU window.

#### Scenario: Replay of a terminal dispatch is rejected (`replay_worker_result_is_receipt_keyed`)
- **GIVEN** a worker dispatch already marked terminal
- **WHEN** a second result is recorded for the same worker grant id
- **THEN** recording is rejected
- **AND** no second `worker.result` event is emitted

### Requirement: Commissioned workers MUST receive a briefcase, not the board (D-085)
A commissioned worker MUST receive a briefcase packed for its own grant and keyed by
its worker grant id. The shared task board MUST NOT be populated by the commissioning
path.

#### Scenario: Briefcase scoped to the worker (`commissioning_persists_briefcase_without_board_row`)
- **GIVEN** a master commissioning a worker
- **WHEN** the worker grant and briefcase are persisted
- **THEN** the briefcase is keyed by the worker grant id
- **AND** no shared board row is written by the commissioning path

