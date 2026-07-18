# Tasks

- [x] Add durable worker dispatch identity, failure state, and connector restart ledger schema.
- [x] Implement atomic `worker.failed` recording with terminal-state and claim cleanup.
- [x] Enforce fresh commission/re-composition restart caps per connector.
- [x] Add grant-id-guarded conversation claim cleanup and identity tuple persistence.
- [x] Wire worker failure handling, cap exhaustion escalation, and action registration.
- [x] Add behavioral tests for failure/re-composition, flaky connector caps, and identity serialization.
- [x] Run the unmasked repository gate and strict OpenSpec validation.
