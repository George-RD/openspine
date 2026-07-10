# Spec: Pipeline driver

## Purpose

Define the kernel's event pipeline as a single typed stage sequence executed by one driver, with per-flow variation expressed as lane data, so that new event flows are added as lane specifications rather than as new hand-rolled driver functions that can drift from the canonical stage order.

## ADDED Requirements

### Requirement: The kernel pipeline MUST be a typed stage sequence the driver executes

The nine pipeline stages — event, verify, identify, route, compose, grant, run, gate, audit — MUST be declared as a typed sequence in exactly one place, with the driver's synchronous prefix (event through run) derived element-by-element from that declaration so the two cannot drift, and the driver's execution MUST be checked against the declared prefix: an instrumented executed-stage trace MUST equal the prefix, pinned by tests for every lane.

#### Scenario: The stage sequence is declared once and pinned

Given the kernel pipeline module
When the stage sequence is inspected
Then it MUST contain the nine stages in the canonical order
And a test MUST pin that order.

#### Scenario: The driver's execution trace matches the declared prefix

Given any lane
When the driver processes an event to completion
Then the executed-stage trace MUST equal the declared synchronous prefix (event, verify, identify, route, compose, grant, run) in order.

### Requirement: Per-flow variation MUST be lane data interpreted by one driver

Everything that differs between event flows — channel trust, lane classification, authority purpose, envelope construction, lane preflight verification, selection-token minting, pending task input, and target route — MUST be captured in a lane specification record, and one driver MUST interpret lane specifications. A lane specification MUST NOT be able to sequence, reorder, or omit stages. Where a lane's variation is behavior, it MUST be a single-stage adapter with typed inputs and outputs: a lane hook MUST NOT resolve routes, compose authority, persist grants, spawn shell runs, or emit audit events belonging to another stage — those remain the driver's alone.

#### Scenario: The owner-control flow is a lane

Given a verified owner Telegram text message
When the driver runs the owner-control lane
Then the flow MUST behave exactly as the previous owner-message handler: same identity resolution channel trust, same authority purpose, same route, same composed grant with the original message ref as pending task input, same shell run, and the same audit events in the same order.

#### Scenario: The selected-thread email preview flow is a lane

Given a verified owner `/draft <thread_id>` command
When the driver runs the email preview lane
Then the flow MUST behave exactly as the previous thread-selection handler: thread existence verified against Gmail, containment guard enforced for the external-communication lane, selection token minted and bound to the grant, the derived pending message persisted and returned as the task's pending input, and the same audit events in the same order.

#### Scenario: A lane cannot skip a stage

Given any lane specification
When the driver executes it
Then every stage of the synchronous prefix MUST run in the declared order
And per-lane absence of stage work (for example no preflight verification in the owner-control lane) MUST be expressed as a no-op input to that stage, not as a skipped stage.

### Requirement: The audited event envelope MUST be emitted only after verification succeeds

The driver MUST emit the `event.received` audit event after the verify stage succeeds, and a preflight-failure exit MUST NOT emit `event.received`, preserving each failure path's existing audit events.

#### Scenario: A preflight failure emits no event envelope

Given a `/draft` command that fails preflight (Gmail not configured, containment refused, thread not found, or a Gmail error)
When the driver's verify stage exits
Then no `event.received` audit event MUST be emitted
And the failure path's existing audit event and owner notification MUST be preserved unchanged.

### Requirement: The driver MUST NOT invoke gate()

The driver's synchronous prefix MUST NOT import or call gate(). Gate execution remains at the effect boundary — the shell's action and model dispatch surfaces and the approval callback — as specified by the gate-action-api capability.

#### Scenario: Driver completes without gating

Given an event that composes a grant and spawns a shell run
When the driver's synchronous prefix completes
Then gate() MUST NOT have been invoked from the driver module
And the gate-mediated dispatch paths specified by the gate-action-api capability remain the only gate call sites.

### Requirement: Lane specifications MUST be compiled-in kernel data

Lane specifications MUST be compiled into the kernel as static constructor values. No public registry, artifact parser, API payload, or runtime mutation path may register, mutate, or remove a lane.

#### Scenario: Lanes have no runtime registration surface

Given the kernel's runtime mutation surfaces (artifact proposal, activation, and the API)
When their inputs are enumerated
Then none MUST accept, register, or alter a lane specification
And the only way to add a lane MUST be a compiled-in constructor reviewed as code.
