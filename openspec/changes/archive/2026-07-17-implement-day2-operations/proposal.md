# Proposal: Implement Day-2 Operations Contract

## Dependencies
- `implement-failure-surfacing-contract` (archived): failure surfacing types and callback acknowledgements.
- `implement-durable-workflow-replay` (archived): durable workflow steps and audit ledger.

## Scope
- This change affects both the **OpenSpine core** substrate (SQLite migrations framework, boot clock regression check, same-conversation serialization) and the **Lyra product** (main bootstrap sequence, failure messages, and recovery procedures).
- It affects system operations, runtime authority verification, and failure surfacing.

## Problem
Currently, the OpenSpine kernel lacks:
1. A versioned database migration path (`PRAGMA user_version`), relying instead on an ad-hoc idempotent `ALTER TABLE` lane that cannot support destructive migrations.
2. Protection against system clock regressions at boot, exposing circuit breakers, token expiries, and audit logs to clock-skew vulnerabilities.
3. Deterministic SQLite I/O-class write failure testing (e.g. read-only or disk-full write errors) to verify that database-level errors are handled correctly by the existing fail-closed audit log propagation and do not invoke connector side-effects.
4. Concurrency serialization for multiple updates targeting the same conversation, causing potential TOCTOU races on briefcase/counter state.

## Proposed Solution
1. **Versioned Migrations Framework**: Introduce a two-lane migration wrapper. The legacy ad-hoc lane executes first to preserve legacy DBs. A `PRAGMA user_version` stamp is committed inside the same transaction as its corresponding DDL, ensuring atomic schema updates. DBs with future version stamps are rejected before any writes or DDL execution.
2. **Boot Clock-Regression Check**: Persist the system clock high-water mark. At boot, compare the system clock; if it has regressed beyond 60s, bail loudly and refuse to start, protecting system invariants.
3. **Audit I/O Verification**: Add a deterministic read-only SQLite test to verify that the existing fail-closed audit log propagation correctly aborts action dispatch (returning 500) and suppresses connector side-effects.
4. **Same-Conversation Serialization**: Refactor the Telegram update handler to extract `chat_id` and acquire an async tokio `Mutex` guard keyed by chat ID, serializing all callbacks and messages for a single conversation.

## Non-Goals
- Multi-user dynamic policy databases (session policy composition remains mock/static).
- Automatic clock correction (the host operator must fix the clock or restore a backup).
- Reverting migrations in production (the downgrade path is test-only).

## Acceptance Criteria
- Legacy databases (user_version 0) are upgraded to latest version with all data preserved.
- Database version and DDL apply atomically, rolling back both if SQL fails.
- Database files with a future schema version are rejected at boot without mutation.
- System clock regressions of >60s prevent the kernel from booting.
- Database-level SQLite write failures (readonly/disk-full) fail the action loudly (500) and halt connector side-effects.
- Multiple callback queries or messages for the same chat are processed sequentially.

## Ratified decisions
This change implements the day-2 operations contract ratified in **AD-139** (versioned migrations, rollback path, consistent backup/restore drill, disk-full loud failure) and **AD-144** (bootstrap posture and same-conversation serialization), recorded in `.raw/openspine-agentos-design-log.md`.
