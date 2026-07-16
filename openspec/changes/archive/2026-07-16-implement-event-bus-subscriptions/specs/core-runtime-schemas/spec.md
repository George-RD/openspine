# Spec: Core runtime schemas (event bus extensions)

## MODIFIED Requirements

### Requirement: Audit schemas MUST reference private payloads by encrypted or hash refs

Audit events MUST store metadata directly.

Private payloads MUST be stored as encrypted artifact refs, hash refs, or
equivalent protected references rather than raw plaintext audit text.

Every audit event schema MUST include a unique event ID, an `aggregate_id`,
and a per-aggregate `aggregate_seq` so consumers can deduplicate and order by
aggregate stream without a parallel event store.

#### Scenario: Model request includes private email content

Given a model request includes private email context
When audit is written
Then raw private content MUST NOT be written directly into the audit event
And the audit event MUST reference protected artifact refs and hashes.

#### Scenario: Audit event carries aggregate stream coordinates

Given an audit event is appended to the ledger
When the persisted audit event is inspected
Then it MUST include a unique event ID
And it MUST include an `aggregate_id` and a positive `aggregate_seq`.

## ADDED Requirements

### Requirement: Event bus subscription types MUST be explicit schemas

OpenSpine MUST define explicit, versioned schema types for event-bus
subscription filters and consumer checkpoints.

A subscription filter MUST be able to constrain audit kind and/or
`aggregate_id`.

A consumer checkpoint MUST record the last successfully handled global ledger
sequence for a named consumer.

#### Scenario: Filter and checkpoint types exist

Given a consumer is configured against the event bus
When its filter and checkpoint are serialized
Then both MUST round-trip through the schema types
And unknown fields MUST be rejected.
