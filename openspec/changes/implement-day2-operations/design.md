# Design: Day-2 Operations Contract

This document presents the detailed design for versioned migrations, clock regression boot detection, audit I/O failure verification, same-conversation serialization, and backup/restore procedures.

---

## 1. Versioned Schema Migrations (`PRAGMA user_version`)

To transition from ad-hoc idempotent migrations to a versioned transactional framework (AD-139), we introduce:

### Two-Lane Migration Design
1. **Ad-hoc Lane (Legacy)**: Remains the baseline (`apply_ad_hoc_migrations`). Runs on every boot to converge any pre-existing database.
2. **Versioned Lane**: Stamped at `BASELINE_USER_VERSION = 1`. Versioned migrations (`VERSIONED_MIGRATIONS`) starting at version `2` are applied sequentially.

### Centralized Initialization & Safety Check
To guarantee that a database from a newer binary is never mutated by an older binary:
- **Immediate Version Check**: We read `PRAGMA user_version` immediately upon opening the SQLite connection. If the version is greater than the latest supported version, the connection bails with `StoreError::UnsupportedVersion` immediately.
- **Centralized Schema DDL**: Base DDL (`SCHEMA_SQL`) is moved inside `apply_versioned_migrations` to run *after* the version check and *before* ad-hoc migrations. This guarantees zero write operations or table creations occur on a newer database file.

### Transactional Atomicity
Each migration's DDL (`up`) and its corresponding `PRAGMA user_version` stamp are executed and committed within a single SQLite transaction:
```rust
let tx = conn.transaction()?;
tx.execute_batch(m.up)?;
tx.execute_batch(&format!("PRAGMA user_version = {}", m.version))?;
tx.commit()?;
```
A failure in DDL execution rolls back both the schema changes and the version stamp.

---

## 2. Boot Clock-Regression Detection

Timestamps and expirations (token validity, breakers, audit chain) trust the system clock. To guard against clock steps (AD-144):
- **Storage**: We introduce a `boot_meta` table (created by versioned migration v2) to persist `clock.high_water_ms` as text. The timer driver records durable runtime observations using the same max-preserving path.
- **NTP Tolerance**: At boot, the system time is compared against the persisted high-water mark. A tolerance of **60 seconds** is subtracted using `saturating_sub` (to prevent integer underflow/panic in debug builds under corrupt or minimum inputs).
- **Control Flow**: Startup first calls a read-only validation. After all fallible setup and the listener bind succeed, it calls an immediate transaction that re-reads/classifies and max-preserving-upserts the candidate. Any regression returns `BootClockCheck::Regressed`; failed startup therefore cannot persist a future candidate.

---

## 3. Same-Conversation Serialization Guard

To serialize concurrent updates for the same Telegram conversation (AD-144):
- **Keyed Lock Registry**: `AppState` gains `conversation_locks: parking_lot::Mutex<HashMap<i64, Arc<tokio::sync::Mutex<()>>>>`.
- **Lock Acquisition**: We acquire an async tokio `Mutex` guard keyed by `chat_id` right after `verify_update` extracts it, and before branching on callbacks or messages:
```rust
let _guard = state.lock_conversation(chat_id).await;
```
The lock is held for the entire duration of `handle_owner_update` (including secret capture, plan callbacks, and sandboxed pipeline runs), ensuring mutual exclusion per conversation.

---

## 4. Audit I/O Verification (Read-Only DB)

Audit logging must fail-closed. To deterministically test this:
- **Test Hook**: We introduce `Store::open_read_only_for_test(path)` which opens the database in read-only mode using `rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY` without initializing the schema.
- **Loud Action Test**: The integration test builds the `AppState` with a writable store (allowing `bootstrap_owner_principal` to run), then replaces `state.store` with a read-only store pointing to the same file. It then dispatches the action. The database write fails with `SQLITE_READONLY` (I/O error), bails with a `500` error, and verifies the mock Telegram connector is never called.

---

## 5. Backup & Restore Scope

A consistent snapshot consists of the entire `data_dir` directory copied while the process is stopped:
- `kernel.db` (database state)
- `artifacts/` (encrypted proposed/validated artifacts)
- `credentials/` (credentials vault)
- `artifacts.d/` (live overlay manifests)

**Decryption Key**: The external `OPENSPINE_ARTIFACT_KEY` AES key must be securely backed up and restored to the running environment alongside this snapshot.

The v2 `down` drops `boot_meta`, including the clock high-water. A later
upgrade must validate the host clock against the restored snapshot (or accept
and document a fresh baseline) before serving. Snapshots always include the
database, blobs, credentials, overlays, and external artifact key as one set.
