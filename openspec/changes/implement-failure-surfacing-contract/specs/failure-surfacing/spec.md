# Failure surfacing

## ADDED Requirements

### Requirement: Durable effect receipts
The kernel MUST propagate an audit append failure as an action failure and MUST NOT report an effect as completed without a durable audit record.

#### Scenario: Audit append failure
- **WHEN** an action's required audit append returns a storage error
- **THEN** the action MUST return failure and MUST NOT continue its effect path

### Requirement: Truthful owner notification
The owner notification path MUST record an attempt before sending and MUST record either `owner.notified` after a successful send or `owner.notify_failed` plus a retryable dead-letter entry after a failed send.

#### Scenario: Connector send failure
- **WHEN** the owner connector returns an error
- **THEN** the audit stream MUST contain `owner.notify_attempted` followed by `owner.notify_failed`, MUST contain a dead-letter entry, and MUST NOT contain `owner.notified` for that attempt

### Requirement: Failure taxonomy routing
Authority and escalation failures MUST route immediately to the owner, while connector and resource failures MUST be batched into the owner digest.

#### Scenario: Connector failure digest
- **WHEN** a connector-class failure is recorded
- **THEN** an owner-authenticated digest retrieval MUST return the failure record

### Requirement: Connector counters
The kernel MUST persist per-connector success and failure counters.

#### Scenario: Counter increment
- **WHEN** a connector succeeds and then fails
- **THEN** its success and failure counters MUST each increase by one


### Requirement: Artifact-backed dead letters
The kernel MUST NOT enqueue a retryable dead-letter without a valid encrypted artifact reference for its owner-facing message; artifact persistence failure MUST produce a plaintext-free durable audit and an owner-retrievable connector-class digest record instead.

#### Scenario: Artifact persistence failure
- **WHEN** connector delivery fails and encrypted artifact persistence also fails
- **THEN** no blank-body dead-letter MUST be enqueued, and the owner-visible digest MUST contain a connector-class failure summary.
