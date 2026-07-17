# Failure surfacing contract

## Dependencies
- AD-138 (settled failure-surfacing contract)
- AD-137 (code-audited evidence)
- AD-082 (leaning owner digest shape)

## Problem/Context
Kernel paths previously discarded audit append errors and marked owner notification as notified before the connector send. Connector failures had no owner-retrievable batch surface or kernel counters.

## Proposed Solution
Make effect receipts durable-or-fail, record owner notification attempt before sending and outcome afterward, enqueue failed sends in a retryable dead-letter table, route authority/escalation failures immediately and connector/resource failures into an authenticated owner digest, and maintain per-connector success/failure counters in SQLite.

## Acceptance Criteria
- No fire-and-forget audit append remains at the audited call sites.
- Injected audit append failure fails the action.
- Injected notification failure records `owner.notify_attempted`, `owner.notify_failed`, and a dead-letter row, without `owner.notified`.
- A connector-class failure appears in the owner-retrievable digest.
- Connector counters persist success and failure outcomes.

## Out of Scope
Digest presentation beyond the minimal owner-retrievable surface; external metrics systems; new decision-log edits.
