# Core runtime schemas

## ADDED Requirements

### Requirement: Worker supervision MUST reset authority on failure (AD-100)
A worker crash MUST atomically terminalize the dispatched worker, emit one structured
`worker.failed` event, and release only that worker's conversation claim. The failed
worker grant MUST NOT be transferred, inherited, or used for continuation. A late
worker result MUST be rejected. Continuation MUST require normal pipeline
re-composition, which creates a distinct grant and dispatch identity.

#### Scenario: Worker crash emits failure and requires re-composition (`worker_crash_emits_structured_failure_and_requires_recomposition`)
- **GIVEN** a commissioned worker with a receipt-bound dispatched row and an in-flight conversation claim
- **WHEN** the worker crashes and a supervisor records the failure
- **THEN** exactly one structured `worker.failed` event is emitted and the row becomes terminal
- **AND** a late result for the failed grant is rejected
- **AND** continuation succeeds only after a fresh commission with a distinct grant and dispatch

### Requirement: Connector restart intensity MUST fail closed (AD-100)
The kernel MUST account restart failures per validated connector in a sliding time
window. Once the connector cap is exhausted, a fresh re-composition/commission MUST
be refused and the owner MUST receive a best-effort escalation notification. Cap
handling MUST NOT mint a grant or restart a worker. Duplicate commission receipts
remain idempotent and return their original result.

#### Scenario: Restart cap holds under a flaky connector (`restart_caps_hold_under_flaky_connector`)
- **GIVEN** a connector with a three-attempt restart cap in a thirty-second window
- **WHEN** three worker attempts fail and a fourth fresh commission is requested
- **THEN** the three failures are recorded and the fourth commission is refused with a fail-closed cap error
- **AND** the restart ledger contains exactly the three charged failures

### Requirement: Worker identity addressing MUST serialize conversations (AD-102)
Worker addresses MUST be the identity tuple `(owner, conversation, task)`, persisted
with the dispatch. The kernel MUST allow at most one in-flight message per
`(owner, conversation)`. Claim release MUST be conditional on the claiming worker
identity so stale cleanup cannot release a newer holder.

#### Scenario: Identity addressing serializes one message per conversation (`identity_addressing_serializes_one_message_per_conversation`)
- **GIVEN** two worker identities in the same owner conversation and a distinct conversation
- **WHEN** both attempt to claim the same conversation and stale cleanup targets the first identity
- **THEN** the second claim is rejected while the first is active, and the distinct conversation may proceed
- **AND** stale cleanup for the first identity cannot remove the second holder
