# Spec delta: core-runtime-schemas (thread_id)

## ADDED Requirements

### Requirement: EventEnvelope MUST carry an optional dormant thread_id

The normalized event envelope (PRD §4.1) MUST include an optional
`thread_id: Option<String>` field. The field defaults to `None` and is
backward-compatible under `deny_unknown_fields` via `serde(default)`. The
field is dormant until a thread-capable channel ships (AD-148); no production
path populates it in v1.

#### Scenario: EventEnvelope without thread_id deserializes as None

- **GIVEN** a serialized EventEnvelope that does not include a `thread_id` key
- **WHEN** it is deserialized
- **THEN** `thread_id` MUST be `None`

#### Scenario: EventEnvelope with thread_id round-trips

- **GIVEN** an EventEnvelope with `thread_id = Some("topic-42")`
- **WHEN** it is serialized and deserialized
- **THEN** `thread_id` MUST equal `Some("topic-42")`

### Requirement: TaskGrant MUST carry an optional dormant thread_id

The task grant (D-007) MUST include an optional `thread_id: Option<String>`
field. The field defaults to `None` and is backward-compatible under
`deny_unknown_fields` via `serde(default)`. The field is a kernel-owned
routing/binding field, not a source of authority, but it MUST be included in
the `RootAuthority` MAC commitment even while dormant so the shell cannot
rewrite the binding. When a thread-capable channel ships, activation begins
populating/using the already authenticated field.

#### Scenario: TaskGrant without thread_id deserializes as None

- **GIVEN** a serialized TaskGrant that does not include a `thread_id` key
- **WHEN** it is deserialized
- **THEN** `thread_id` MUST be `None`

#### Scenario: TaskGrant with thread_id round-trips

- **GIVEN** a TaskGrant with `thread_id = Some("topic-42")`
- **WHEN** it is serialized and deserialized
- **THEN** `thread_id` MUST equal `Some("topic-42")`

#### Scenario: Mutating thread_id invalidates the grant MAC

- **GIVEN** a sealed TaskGrant with `thread_id = None`
- **WHEN** its `thread_id` is changed to `Some("topic-42")` without resealing
- **THEN** MAC verification MUST fail
