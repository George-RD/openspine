# Design: OTP-style supervision with authority reset

## Failure terminal transition
`store::worker_supervision::record_worker_failed` starts `BEGIN IMMEDIATE`, reads the
worker dispatch and trusted connector/identity binding, and rejects any non-dispatched
row. It charges the connector restart ledger, flips the row to `terminal` with
`failed_at` and the cap count, and appends the structured `worker.failed` payload before
commit. A concurrent `record_worker_result` races on the same guarded state flip; only
one terminal outcome can commit. The failed worker's in-flight conversation claim is
released with a grant-id guard in the same transaction.

## Authority reset and continuation
Failure handling has no grant-minting path. `worker.report_result` against a failed
dispatch is rejected by the existing terminal receipt guard. A normal pipeline
re-composition calls the existing worker commission path with a newly minted worker
grant and a distinct dispatch identity; the failure event only reports
`recomposition_permitted`, it never respawns or transfers the dead grant.

## Restart-intensity accounting
`connector_restart_ledger` stores `(connector, occurred_at)` rows. The default cap is
three failures/re-compositions in thirty seconds. The connector key is looked up from
the commissioned worker's validated route in the kernel registry and persisted on the
dispatch row; it is not accepted from the commission payload. Once the cap is exceeded,
the event is marked `recomposition_permitted: false` and the handler surfaces a
fail-closed owner notification.

## Identity addressing and serialization
`WorkerIdentity` is a serializable `(owner, conversation, task)` tuple with no process
handle. The dispatch persists its fields. `conversation_in_flight` has a primary key on
`(owner, conversation)`; acquisition is a single atomic insert and a duplicate claim is
rejected. Handler RAII cleanup releases claims on every terminal handler path, while
failure cleanup uses the worker grant id so a newer claim cannot be deleted.

## Tests
- `worker_crash_emits_structured_failure_and_requires_recomposition`
- `restart_caps_hold_under_flaky_connector`
- `identity_addressing_serializes_one_message_per_conversation`
