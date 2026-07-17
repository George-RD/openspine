# failure-surfacing Specification

## Purpose
TBD - created by archiving change implement-failure-surfacing-contract. Update Purpose after archive.
## Requirements
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


### Requirement: Direct authenticated bad-request surfacing
An authenticated API bad request MUST be returned directly to its caller and MUST NOT also invoke owner notification for the same failure. Failures outside that direct response boundary MUST continue to use the immediate or digest failure lane selected by their taxonomy.

#### Scenario: Authenticated bad request
- **WHEN** an authenticated caller submits a malformed or unsupported action request
- **THEN** the API MUST return a typed bad-request response and MUST NOT emit a duplicate owner notification

### Requirement: Delivery-unknown crash semantics
The runtime MUST persist an owner-notification attempt before external send and MUST commit receipt completion transactionally under the matching claim token after confirmed send. A committed receipt MUST prevent retry. If the process crashes after provider send but before receipt commit, recovery MAY resend and MUST NOT claim exactly-once delivery.

#### Scenario: Crash before receipt commit
- **WHEN** the provider accepts an owner notification but the runtime crashes before its durable receipt commits
- **THEN** the notification MUST remain eligible for fenced retry and its delivery state MUST remain delivery-unknown

### Requirement: Secure lossless digest pagination
Authenticated owner digest detail retrieval MUST reconstruct every retained UTF-8 byte from encrypted stable references using deterministic bounded pages with stable item identity and page N/M. Successful page delivery MUST record a detail-specific receipt. Missing, corrupt, or undecryptable detail MUST remain unresolved and truthfully audited without leaking plaintext.

#### Scenario: Paginated detail reconstruction
- **WHEN** the authenticated owner retrieves every page for a retained digest item
- **THEN** concatenating the page payloads MUST reproduce the original UTF-8 bytes exactly and each page MUST remain within the connector message bound

#### Scenario: Unavailable encrypted detail
- **WHEN** a digest detail reference is missing, corrupt, or cannot be decrypted
- **THEN** retrieval MUST fail closed, MUST NOT record a successful detail receipt, and MUST emit a truthful resource failure without plaintext
