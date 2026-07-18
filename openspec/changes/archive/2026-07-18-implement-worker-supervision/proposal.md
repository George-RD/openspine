# Proposal: Worker supervision with authority reset

## Dependencies
- Landed worker runtime commissioning, receipt-bound dispatch, fail-closed recovery, and result relay (D-100..D-103).
- Existing Store audit ledger, task-grant persistence, action catalog, and failure-surfacing path.
- Canon AD-100 and AD-102 (identity addressing leaning); D-012 encrypted artifact references.

## Problem
A worker shell can crash after commissioning, but the runtime needs an explicit terminal
failure transition that cannot be confused with a late result. Restarting from a dead
worker's bearer grant would transfer authority across task instances. Without a durable
failure event, connector-scoped restart cap, and identity-scoped serialization, flaky
connectors can cause restart storms and same-conversation messages can race.

## Proposed Solution
Add kernel-owned supervision state around `worker_dispatch`. A crash atomically competes
with `worker.result` for the dispatched-to-terminal transition and emits one structured
`worker.failed` audit event. The failure event records the identity tuple, trusted
connector bucket, cap decision, and optional encrypted diagnostic reference. Failure
handling never mints or transfers authority; a continuation must return through normal
composition and receive a fresh grant. Connector failures use a sliding restart ledger
with a three-attempts-per-thirty-seconds default cap and owner notification on exhaustion.
Workers are addressed by `(owner, conversation, task)`, while a unique in-flight claim at
`(owner, conversation)` serializes messages atomically and is reclaimed on crash.

## Acceptance Criteria
- A crash records exactly one structured `worker.failed` event and terminalizes the dead dispatch.
- A late result for the failed grant is rejected; only a fresh commission/re-composition creates a new dispatch and grant.
- The restart cap permits the configured attempts and denies the next flaky-connector attempt.
- Identity addressing uses `(owner, conversation, task)` and one in-flight message per `(owner, conversation)`.
- Connector cap keys are derived from the commissioned worker's validated route, never caller payload text.
- `cargo test`, `cargo clippy -D warnings`, `cargo fmt --check`, file-size, claims, ceremony, and strict OpenSpec validation pass.

## Out of Scope
- Clustering or a distributed actor registry.
- Automatic grant minting or worker replacement inside failure handling.
- Connector circuit-breaker policy beyond restart-intensity accounting.
