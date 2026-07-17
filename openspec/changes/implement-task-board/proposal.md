# Proposal: Implement the kernel task board

## Why

Tasks and commitments must survive context compaction and process restarts as kernel-owned state. The master agent needs bounded workload slices instead of a board-sized context, and deadline/reminder time must enter the same authority pipeline as other events.

## What Changes

- Add a SQLite-backed task/commitment store for status, owning worker and grant, due/reminder timing, dependencies, and non-sensitive provenance references.
- Associate task deadlines and reminders with the archived durable workflow timer registry. Timer firing remains kernel-owned and at-most-once, then dispatches the resulting timer event through normal event, route, grant, and gate boundaries.
- Add deterministic bounded read-model projections for due-now, blocked, and asked-about work. Master-facing APIs return slices only; task detail and whole-board rows remain in the store.
- Ship deterministic slice ordering and limits. AD-123 hysteresis scoring remains deferred.

## Acceptance Criteria

- A due task produces one `workflow.timer_fired` event and the scheduled task is routed, granted, and gated under test.
- Master-facing reads return bounded due-now, blocked, and asked-about slices without task-detail payloads or a whole-board result.
- Rust formatting, lint, tests, file-size checks, and strict OpenSpec validation are green.
