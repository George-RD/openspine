# Durable workflow replay

## ADDED Requirements

### Requirement: Workflow steps replay by exact durable handle
The kernel MUST append a Pending outbox intent before an outside-world step and MUST identify receipts/completions by the exact StepHandle and stored aggregate sequence. Recovery MUST verify and replay through the event-bus validator under one snapshot and MUST NOT invoke a recorded operation again.

#### Scenario: Crash recovery preserves state without duplicate effects
- **WHEN** a run records connector and model outcomes, is killed, and is rehydrated
- **THEN** replay returns the recorded outcomes, the final state matches an uninterrupted baseline, and operation side-effect counters do not increase

#### Scenario: Pending and Resuming are durable states
- **WHEN** a process is killed after Pending is committed and before Completed is committed
- **THEN** recovery returns Resuming with the stable idempotency key and MUST NOT redispatch the effect automatically

### Requirement: Failures are terminal and replayable
Dispatch, artifact, receipt, and completion failure branches MUST attempt a terminal Outcome Err. Completion append errors MUST propagate. A successful terminal failure return MUST equal the persisted and replayed closed non-sensitive error code; recovery MUST NOT leave silent Pending.

#### Scenario: Failed provider outcome is durable
- **WHEN** an async provider returns an error
- **THEN** the error code is appended and a recovered run returns the same recorded error without invoking the provider again

### Requirement: Gated and approval identity is bound
Gated replay identity MUST bind action, grant, bound chat, actual payload digest, and step-input digest. Raw dispatch MUST be private and all execution MUST route through the mediator. Generic approval-kind rows MUST be rejected; the typed approval adapter MUST require action plus typed target and payload digests.

#### Scenario: Stale gated identity diverges
- **WHEN** a replayed gated step receives a changed payload or bound chat
- **THEN** the digest diverges before dispatch, and the raw dispatcher cannot be called directly

#### Scenario: Generic approval rows are rejected
- **WHEN** a caller tries to begin an approval-kind step through generic `begin_step`
- **THEN** the kernel rejects it, while the typed adapter accepts only action plus non-optional target and payload digests

### Requirement: Private payloads do not leak

Inline payloads MUST be a sealed closed non-secret set. Private success values MUST be stored as protected ArtifactRefs; private failures MUST be represented only by a closed non-sensitive code or protected reference, never plaintext.

#### Scenario: Private failure has no plaintext
- **WHEN** a private step fails with a provider message
- **THEN** the ledger stores only a closed non-sensitive code or protected reference, and replay returns no plaintext message

### Requirement: Timers have atomic durable transitions
Timer scheduling MUST CAS the exact StepHandle and append the canonical Completed record plus timer registry row atomically. Concurrent contexts MUST return the same TimerSpec. Firing MUST use trusted current time and a database `fires_at <= now` predicate, and MUST append at most one `workflow.timer_fired` event.

#### Scenario: Dark-window timer fires once
- **WHEN** a timer is scheduled, polled before its deadline, and two recovered contexts poll after its deadline
- **THEN** the pre-deadline poll emits no event, both contexts observe one canonical TimerSpec, and the timer aggregate contains exactly one firing event

## Ratified decisions

The parent adjudicated this change's candidate decision text as **D-073** (durable workflow steps persist intent before effect; recovery replays recorded outcomes and fails closed on receiptless pending non-idempotent effects; sealed inline payload set) and **D-074** (kernel-owned workflow timers fire at most once via trusted-clock atomic claims), recorded in `.raw/openspine-decision-log.md`.
