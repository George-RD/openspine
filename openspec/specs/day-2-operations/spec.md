# day-2-operations Specification

## Purpose
TBD - created by archiving change implement-day2-operations. Update Purpose after archive.
## Requirements
### Requirement: Versioned schema migrations
The database store MUST implement versioned migrations using `PRAGMA user_version`. On database open:
1. The store MUST read `user_version`. If the version is newer than the binary supports, opening MUST fail immediately with `UnsupportedVersion` before executing any DDL or modifying the database.
2. The legacy ad-hoc migration lane MUST run first to converge older database schemas to the baseline.
3. Versioned migrations MUST be applied in order, and each migration's DDL and its corresponding version stamp MUST be committed within a single atomic SQLite transaction. If SQL fails, both the DDL changes and the version stamp MUST roll back.
4. A test-only downgrade helper MUST roll back versioned migrations in reverse order, applying each `down` DDL and updating the version stamp transactionally.

#### Scenario: Transactional migration rollback on DDL failure
- **WHEN** a versioned migration runs SQL that fails
- **THEN** the transaction rolls back, the database schema remains unchanged, and `PRAGMA user_version` is not updated

#### Scenario: Future version database is rejected unmutated
- **WHEN** a database has a version stamp newer than supported
- **THEN** the store bails with `UnsupportedVersion` and zero tables or schema alterations are executed

---

### Requirement: Boot clock-regression detection
The database store and boot sequence MUST protect against backwards clock steps:
1. The kernel MUST persist the highest observed wall-clock timestamp as `clock.high_water_ms` in the `boot_meta` table.
2. At boot, if the current system time is behind the persisted high-water mark by more than 60 seconds (NTP tolerance), the kernel MUST fail to bootstrap, log a loud error, and bail.
3. The persisted clock high-water mark MUST NOT be lowered by a regression or boot failure.

#### Scenario: Clock regression beyond tolerance bails loudly
- **WHEN** the system time at boot is behind the persisted high-water by 61 seconds
- **THEN** the kernel refuses to start, returns an error, and the persisted high-water remains unchanged

#### Scenario: NTP adjustments within tolerance are accepted
- **WHEN** the system time at boot is behind the persisted high-water by 30 seconds
- **THEN** the check passes, the kernel boots, and the high-water is not lowered

#### Scenario: Runtime observations survive restart
- **WHEN** the running timer driver records a later wall-clock observation and the process restarts
- **THEN** the observation is durable and a boot more than 60 seconds behind it is rejected

#### Scenario: Startup failure does not commit candidate
- **WHEN** startup validates a future candidate but a later fallible setup or bind step fails
- **THEN** the candidate is not persisted, and a retry validates against the prior high-water

#### Scenario: Clock classification and update are serialized
- **WHEN** concurrent stores attempt clock updates with different timestamps
- **THEN** each read/classify/upsert is an immediate transaction and the persisted value is the maximum observed timestamp

#### Scenario: Downgrade drops clock high-water
- **WHEN** the test-only v2 down migration is applied
- **THEN** `boot_meta` and its high-water are dropped, and the next upgrade requires host-clock/backup validation before serving

---

### Requirement: Same-conversation serialization
Concurrent Telegram updates targeting the same conversation (chat ID) MUST be serialized:
1. The update handler MUST extract `chat_id` and acquire an async tokio `Mutex` guard keyed by chat ID before any message or callback processing occurs.
2. The lock MUST be held for the entire processing duration (including secret capture, plan callbacks, and sandboxed pipeline runs).

#### Scenario: Concurrent callbacks are executed sequentially
- **WHEN** two callback queries are dispatched concurrently for the same Telegram chat
- **THEN** the second callback query blocks at the conversation lock and is processed only after the first callback query completes

---

### Requirement: Audit I/O failure handling
The kernel and action dispatch pipeline MUST fail closed under database-level write failures:
1. Database-level I/O write failures (such as disk-full `SQLITE_FULL` or read-only `SQLITE_READONLY` connections) MUST propagate from the store.
2. An audit logging failure during action dispatch MUST return a `500` error and halt execution before any connector side-effects are dispatched.

#### Scenario: Read-only database write failure aborts action
- **WHEN** an action is posted to the kernel while the database connection is read-only
- **THEN** the audit append fails with an I/O error, the HTTP response returns `500`, and no Telegram reply or connector side-effect is sent

#### Scenario: Disk-full audit append aborts before connector effect
- **WHEN** the action audit append encounters `SQLITE_FULL`
- **THEN** the action returns a server error and no connector effect is executed or reported as completed

---

### Requirement: One-set snapshot and restore
A day-two backup MUST be requested through the non-delegable root-owner `openspine.overlay.export` action using a bounded name inside the kernel-controlled snapshot root and completed under the canonical data-directory lifetime lock on restart before stores open. It MUST atomically publish one authenticated directory bundle containing the complete configured `data_dir` at-rest representation (`kernel.db`, `artifacts/`, `keys/`, `credentials/`, and `artifacts.d/`), an exact typed-tree manifest, restrictive `0700`/`0600` modes, and a signed terminal-erasure-ledger baseline. The external `OPENSPINE_ARTIFACT_KEY` and latest signed terminal-erasure ledger MUST be preserved separately for portable recovery.

Restore MUST be requested through the equivalent root-owner action after a bundle is staged in the protected snapshot root. It MUST copy-hash/validate the exact typed tree into same-filesystem staging, merge non-regressing erasure continuity, install through crash-recoverable new/old stages, and retain the previous generation until the complete normal startup contract passes. Migration, owner bootstrap, audit-chain verification, clock and erasure reconciliation, overlay compatibility/admission, provider/connector checks, listener bind, and post-bind clock commit MUST pass before serving or cleanup. A failed installed restore MUST support authenticated pathless offline rollback.

#### Scenario: One authenticated snapshot restores coherently
- **WHEN** the verified root owner gates a named export, restarts to publish it, separately preserves the exact artifact master key and latest signed erasure ledger, stages/gates a restore, and restarts
- **THEN** one authenticated point-in-time data generation is installed with restrictive permissions, terminal erasures remain closed, all startup/compatibility checks pass before serving, and old data is removed only after auditable finalization

#### Scenario: Portable restore lacks external continuity
- **WHEN** a transferred bundle reaches a fresh host without the matching external master key or a signed terminal-erasure ledger at least as new as its embedded baseline
- **THEN** restore fails before moving active data

#### Scenario: Installed generation fails a late startup check
- **WHEN** a restored generation fails provider validation, listener bind, or post-bind clock commit
- **THEN** the signed pending restore and previous generation remain available and the documented authenticated offline rollback restores the prior generation before serving

---

### Requirement: Telegram-first first-run sequence
First-run documentation MUST describe the actual AD-144 operator sequence: seed/read the Telegram bot token, verify the configured owner sender in a private chat, then configure Gmail OAuth out of band. The implementation MUST keep Gmail optional for Telegram-only operation and perform OAuth token exchange lazily on a Gmail operation; it MUST preserve exact failure messages from config, Telegram verification, and Gmail token refresh.

#### Scenario: Documented operator sequence keeps OAuth lazy
- **WHEN** an operator starts with a valid bot token, sends a private Telegram message from the configured owner, and later performs a Gmail operation
- **THEN** the bot token is read or seeded first, only the matching private-chat sender receives owner authority, and Gmail OAuth token exchange occurs only for that Gmail operation

#### Scenario: First-run failures remain actionable
- **WHEN** the bot token is missing, the Telegram sender is not the configured private-chat owner, or Gmail credentials/token refresh are missing or rejected
- **THEN** the operator sees the exact corresponding messages `missing required environment variable OPENSPINE_TELEGRAM_BOT_TOKEN`, the audited reason codes (`unknown_telegram_user`, `no_sender`, `non_text_update`, `callback_query_missing_data`, or `owner_message_outside_private_chat`), or `gmail token refresh failed: HTTP 0: gmail client secret is not configured`, `gmail token refresh failed: HTTP 0: gmail refresh token is not configured`, or `gmail token refresh failed: HTTP <status>: <body>`

