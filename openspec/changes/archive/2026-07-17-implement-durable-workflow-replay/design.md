# Design: Durable workflow replay

## Ledger aggregate and verified replay

Each run uses `workflow_run:{run_id}`. Rehydration calls one store helper that verifies the global hash chain and then invokes the event-bus replay validator while the same connection snapshot is held. Aggregate sequence gaps, malformed payloads, metadata mismatches, and event/row coordinate mismatches fail closed.

## Exact step identity

`begin_step` computes the canonical input digest and creates an exact `StepHandle { step_id, pending_seq }` for the durable `Pending` intent. Receipt and completion writes require that handle and use the registry's stored sequence. Physical append order may interleave concurrent steps; replay pairs records by step ID and exact input digest. A completed row returns `Replayed`; a pending-only row returns `Resuming` with its stable idempotency key.

## Timer substrate

`WorkflowCtx::schedule_timer` begins the timer step, then calls one SQLite immediate transaction. The transaction validates the exact pending sequence, CAS-claims the step's completion slot, appends the canonical `Completed` record, and inserts the pending timer row before commit. A losing context reads and returns the winner's stored `TimerSpec`. Firing uses the kernel's trusted current time and `fires_at <= now` in the database status CAS; stale and early callers cannot fire a timer, and the terminal event is returned idempotently. Task-board and standing-rule consumers can subscribe later.

## Gated, approval, and private boundaries

Gated replay binds action, grant, bound chat, actual payload digest, and step-input digest, and calls only the mediator; raw dispatch is private. Dispatch, artifact, receipt, and completion failures persist one closed non-sensitive terminal code, propagate any completion append error, and recovery never redispatches a gated Pending step without a receipt. Approval-kind rows are rejected by generic `begin_step`; the typed adapter requires action, target digest, and payload digest and uses an allowed output type. Inline payloads are a sealed non-secret set. Private success values use encrypted `ArtifactRef`s and private failures use only a closed non-sensitive code.

## Ratified decisions

The parent adjudicated this change's candidate decision text as **D-073** (durable workflow steps persist intent before effect; recovery replays recorded outcomes and fails closed on receiptless pending non-idempotent effects; sealed inline payload set) and **D-074** (kernel-owned workflow timers fire at most once via trusted-clock atomic claims), recorded in `.raw/openspine-decision-log.md`.
