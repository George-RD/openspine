# Tasks

- [x] Add explicit workflow aggregate append path on audit ledger.
- [x] Add exact StepHandle Pending, Receipt, and Completed boundaries.
- [x] Add deterministic kernel-mediated time and randomness helpers.
- [x] Add atomic timer StepHandle CAS and canonical TimerSpec convergence.
- [x] Add durable dispatch, artifact, receipt, and replayable failure outcomes.
- [x] Bind gated payload/chat identity and enforce mediator-only dispatch.
- [x] Add typed approval adapter and generic approval-kind rejection.
- [x] Seal inline payloads and prevent private failure plaintext.
- [x] Verify audit and replay through one validated snapshot.
- [x] Add concurrency, trusted-time, Pending/Resuming crash, and recovery tests.
- [x] Restore D-055 catalog/timer effect-path assertions.
- [x] Document actual StepHandle/CAS/ref-backed semantics and candidate decision text.

## Ratified decisions

The parent adjudicated this change's candidate decision text as **D-073** (durable workflow steps persist intent before effect; recovery replays recorded outcomes and fails closed on receiptless pending non-idempotent effects; sealed inline payload set) and **D-074** (kernel-owned workflow timers fire at most once via trusted-clock atomic claims), recorded in `.raw/openspine-decision-log.md`.
