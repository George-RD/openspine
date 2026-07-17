# Tasks: Implement Day-2 Operations Contract

## Implementation
- [x] Create the `boot_clock` module implementing clock regression detection and high-water updates.
- [x] Add the versioned SQLite migration framework in `migrations.rs` with `BASELINE_USER_VERSION = 1` and v2 migration.
- [x] Implement transactional schema/version updates in `migrations.rs` and the `revert_versioned_migrations_for_test` helper.
- [x] Add `UnsupportedVersion` error to `StoreError` and reject newer DB version files before running schema DDL in `mod.rs` / `migrations.rs`.
- [x] Implement same-conversation serialization using tokio async locks in `AppState` and `handle_owner_update`.
- [x] Add `open_read_only_for_test` hook to `test_hooks.rs`.
- [x] Wire the boot clock check in `main.rs` and reject starting when clock regresses >60s.
- [x] Persist runtime clock heartbeats through the timer driver and test restart durability.
- [x] Split startup clock validation from post-bind max-preserving commit.

## Verification
- [x] Add migration tests for up/down atomic rollback, legacy DB upgrade, and future version rejection in `migration_tests.rs`.
- [x] Add unit tests for clock regression (NTP tolerance, saturating underflow) in `day2_tests.rs`.
- [x] Add integration test for same-conversation lock serialization using a controllable callback responder gate in `pipeline/tests/concurrency.rs`.
- [x] Add integration test for readonly database write failure (audit I/O) loud action failure in `pipeline/tests/effect_paths.rs`.
- [x] Add deterministic runtime restart, max-preserving concurrency, and startup commit-order tests.
- [x] Use a valid `TelegramReplyPayload { text }` in the read-only and disk-full connector tests.
- [x] Document the backup/restore drill and failure messages in `docs/day-2-operations.md`.
- [x] Run cargo fmt, clippy, test, file-size check, and openspec validate.
