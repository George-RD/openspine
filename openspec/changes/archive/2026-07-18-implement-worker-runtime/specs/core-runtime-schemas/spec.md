# Core runtime schemas

## ADDED Requirements

### Requirement: Worker sub-grants MUST be offline-verifiable caveat-chain attenuations
A commissioned worker's grant MUST be a caveat-chain child of the commissioning
master's own grant, where the child's MAC extends the parent's sealed tip. The child
MUST verify offline against only the HMAC key and its embedded chain, with no parent
or root database lookup. A child MUST never widen the parent's authority.

#### Scenario: Offline verification of a multi-level chain (`offline_chain_verify_multi_level`)
- **GIVEN** a master grant and a worker sub-grant minted from it, and a second worker
  sub-grant minted from that worker
- **WHEN** the chain is verified using only the HMAC key and the child's embedded chain
- **THEN** verification succeeds without any store or network access

#### Scenario: Widening action is rejected (`child_cannot_widen_parent_action`)
- **GIVEN** a master grant whose effective allowed actions do not include `email.send_draft`
- **WHEN** a worker sub-grant is minted requesting `email.send_draft`
- **THEN** minting is rejected
- **AND** no child grant is produced

#### Scenario: Widening expiry is rejected (`child_cannot_widen_parent_expiry`)
- **GIVEN** a master grant with an effective expiry
- **WHEN** a worker sub-grant is minted with an expiry later than the master's
- **THEN** minting is rejected

### Requirement: Master MUST commission workers and relay results as bus events
A master agent MAY commission a worker by minting a narrowed sub-grant of its own
grant and packing the worker's briefcase. The worker's result MUST return as a
`worker.result` bus event on the worker grant's aggregate; the master MUST consume it
through the ordinary event bus and relay through its own separately-gated reply path.

#### Scenario: Worker commissioned and result consumed (`result_is_consumed_bus_event`)
- **GIVEN** a master grant authorized for `worker.commission`
- **WHEN** the master commissions a worker and the worker reports a result
- **THEN** a single `worker.result` bus event is recorded on the worker grant's
  aggregate carrying the structured payload
- **AND** the master consumes it through the normal event-bus path


#### Scenario: Parent allows action but worker denies it (`worker_denied_outside_narrowed_allowlist`)
- **GIVEN** a parent grant allows `openspine.status.read` and the worker grant narrows that action away
- **WHEN** the parent and worker requests for `openspine.status.read` are evaluated at the gate
- **THEN** the parent request is allowed
- **AND** the worker request is denied
#### Scenario: Worker report action remains allowed (`worker_allowed_exact_report_action`)
- **GIVEN** a worker grant is narrowed to the exact `worker.report_result` action
- **WHEN** the worker report request is evaluated at the gate
- **THEN** the gate decision is `Allow`

#### Scenario: Classified empty output channel is denied (`empty_declared_output_channels_fail_closed`)
- **GIVEN** the action catalog classifies `test.output` as an output action with an empty channel vector
- **WHEN** a grant allowing `test.output` is evaluated at the gate
- **THEN** the gate denies with `OutputChannelNotGranted`
### Requirement: Worker grants MUST have no effective output channel (reply chokepoint)
A commissioned worker grant MUST carry an empty `OutputChannelAllowlist` caveat so its
effective output channels are provably empty regardless of the root's channel list. A
worker MUST NOT egress directly; its only outbound path is `worker.report_result`.

#### Scenario: Worker cannot egress directly (`classified_empty_output_channel_denial`)
- **GIVEN** a root grant that carries an output channel
- **WHEN** a worker sub-grant is minted from it
- **THEN** the worker's effective output channels are empty
- **AND** `effectively_allows_output_channel` returns false for every root channel

### Requirement: Worker result recording MUST be receipt-keyed and fail-closed (D-083)
Recording a worker result MUST mark the dispatch terminal and MUST reject a second
result for an already-terminal dispatch, never replaying it. The terminal flip and the
receipt check MUST share no TOCTOU window.


#### Scenario: Replay of a terminal dispatch is rejected (`replay_worker_result_is_receipt_keyed`)
- **GIVEN** a worker dispatch already marked terminal
- **WHEN** a second result is recorded for the same worker grant id
- **THEN** recording is rejected
- **AND** no second `worker.result` event is emitted

### Requirement: Commissioned workers MUST receive a briefcase, not the board (D-085)
A commissioned worker MUST receive a briefcase packed for its own grant and keyed by
its worker grant id. The shared task board MUST NOT be populated by the commissioning
path.

#### Scenario: Briefcase scoped to the worker (`commissioning_persists_briefcase_without_board_row`)
- **GIVEN** a master commissioning a worker
- **WHEN** the worker grant and briefcase are persisted
- **THEN** the briefcase is keyed by the worker grant id
- **AND** no shared board row is written by the commissioning path
