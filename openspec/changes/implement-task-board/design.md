# Design: Kernel task board

## Task objects and persistence

`openspine_schemas::task` defines deny-unknown-fields `Task`, `TaskStatus`, `TaskTimerKind`, `TaskProvenance`, and bounded `TaskSlice` DTOs. Task detail uses validated `ArtifactRef` values for title/provenance references, never plaintext sensitive text. `store/task_board.rs` persists canonical `task_json` and extracts status, timestamps, owner, title ref, provenance JSON, and timer IDs for deterministic SQLite queries.

## Timers and normal authority path

Task deadline/reminder timer IDs are linked to the existing `workflow_timers` registry. Scheduling writes the registry row and `workflow.timer_scheduled` ledger event in one immediate transaction. The existing `workflow::run_timer_driver` remains the sole timer firer and emits `workflow.timer_fired` at most once. A separate task-board consumer replays only new `workflow.timer_fired` events through the event-bus watermark; it acknowledges only after the scheduled `Event → Verify → Identify → Route → Compose → Grant → Run` path succeeds. The worker receives a grant and acts through the existing mediator/gate boundary; the timer handler never performs a worker effect.

Deadline and reminder events have distinct event types and precise routes, preventing unrelated scheduled-internal timers from entering the task-board lane. The scheduled lane supplies the task owner's principal as a kernel/task authority identity, not synthetic Telegram proof.

## Bounded master read-model

`Store::master_slice(now, limit)` deterministically combines due-now, blocked, and asked-about projections, de-duplicates by task ID, and caps the result. The scheduled lane's pending artifact contains only serialized `TaskSlice` DTOs (fixed cap 10). Full task JSON remains available only through the kernel task store and is never copied into master context. AD-123 hysteresis scoring is explicitly deferred; selection is deterministic category ordering.

## Failure and replay behavior

Non-task timer events are acknowledged and skipped. Terminal task statuses are acknowledged and skipped to avoid stale timer dispatch. A task pipeline error leaves the event-bus checkpoint unchanged, so the event is retried. D-012 is enforced by schema types and the canonical JSON validation boundary.

## Timer dispatch invariants

`TimerDispatchOutcome` distinguishes `Delivered`, `AckSkip`, and `Retry`. The fired audit-event ID is inserted into `processed_timer_events` in the same SQLite transaction as the grant. Unknown owners and unmet dependencies are permanent skips; unmet dependencies atomically transition the task to `Blocked` and append `task.blocked`. Retryable authority/store failures withhold the consumer checkpoint.

The scheduled routes use `task_board_scheduled` plus `scheduled_timer_pack`; the pack constrains applicability to `scheduled_internal`, retains owner-channel reply/model/workflow actions, and denies unrelated `artifact.propose`. The typed `EventInputs.correlated_task_id` anchors the redacted bounded slice without encoding kernel state into presentation text.
