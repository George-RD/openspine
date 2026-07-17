# Tasks

- [x] Add validated task-board schemas with round-trip and unknown-field tests.
- [x] Add SQLite task store with canonical JSON and extracted indexes.
- [x] Link deadline and reminder timers transactionally to workflow timers.
- [x] Add precise scheduled deadline and reminder route/event handling.
- [x] Consume timer-fired events through durable event-bus checkpoints.
- [x] Deliver bounded due, blocked, and asked-about master slices.
- [x] Test worker gate handoff after routed task deadline.
- [x] Add timer idempotency, owner/dependency validation, blocked attention, and retry classification.
- [x] Add dedicated scheduled workflow/pack and applicability enforcement.
- [x] Run all required local gates and record evidence.
- Gate evidence: fmt, clippy `-D warnings`, workspace tests, file-size check, and strict OpenSpec validation all pass in `IMPLEMENTATION-NOTES.md`.
