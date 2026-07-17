# task-board Specification

## Purpose
TBD - created by archiving change implement-task-board. Update Purpose after archive.
## Requirements
### Requirement: Tasks are durable kernel objects

The kernel MUST persist tasks and commitments as validated schema objects containing status, owning worker/grant, due and reminder timing, dependencies, provenance references, and stable identity. Persisted task JSON MUST use the schema validation boundary and MUST NOT contain plaintext secrets or sensitive text.

#### Scenario: Task lifecycle round-trips

- **WHEN** a task with blocked status, dependencies, owner grant, due time, reminder time, and ArtifactRef provenance is written and read
- **THEN** every field, including status, dependencies, timer IDs, and provenance kind, MUST round-trip without widening or silently dropping unknown fields

#### Scenario: Sensitive task text is rejected from refs

- **WHEN** a caller constructs task title or provenance fields
- **THEN** the schema MUST require validated ArtifactRef values rather than accepting arbitrary plaintext strings

### Requirement: Deadline and reminder timers use the normal pipeline

Task deadlines and reminders MUST be linked to the archived durable workflow timer registry. Scheduling MUST atomically append `workflow.timer_scheduled` with the registry row. The existing kernel timer driver MUST emit at most one `workflow.timer_fired` event, and task-board consumption MUST replay that event through normal event, verification, identity, route, composition, grant, and worker-run stages. The timer handler MUST NOT perform a direct worker effect.

#### Scenario: Deadline reaches routed and granted worker

- **WHEN** a linked deadline timer becomes due and the kernel timer driver fires it
- **THEN** exactly one `workflow.timer_fired` event MUST be replayed by the task-board consumer, the precise deadline route MUST resolve, and an owner-bound worker grant MUST be persisted

#### Scenario: Reminder uses the same path

- **WHEN** a linked reminder timer becomes due
- **THEN** the reminder event MUST use its precise reminder route and the same normal pipeline, without being treated as an unrelated scheduled-internal timer

#### Scenario: Timer event replay retries before acknowledgement

- **WHEN** the scheduled pipeline fails after a `workflow.timer_fired` append
- **THEN** the task-board event-bus checkpoint MUST remain unchanged and a later replay MUST retry the event

### Requirement: Master receives bounded task slices

The kernel MUST expose deterministic bounded due-now, blocked, and asked-about projections. The master-facing slice MUST contain only bounded `TaskSlice` DTOs and MUST NOT include full task detail, dependencies, provenance, or owning grant data. AD-123 hysteresis attention scoring is out of scope for this deterministic slice.

#### Scenario: Master slice is capped and detail-free

- **WHEN** the board contains more tasks than the requested slice cap
- **THEN** the returned due-now + blocked + asked-about slice MUST contain no more than the cap and serialized slice rows MUST omit full-task fields

#### Scenario: Terminal task timer is skipped

- **WHEN** a completed or cancelled task's linked timer event is replayed
- **THEN** the consumer MUST acknowledge and skip it without launching a worker grant

### Requirement: Timer dispatch is idempotent and classified

The timer consumer MUST record the fired audit-event id in the same transaction as the worker grant. Replays of a processed event MUST acknowledge without creating another grant. Outcomes MUST distinguish delivered, permanently skipped, and retryable processing.

#### Scenario: Replayed timer event does not duplicate a grant

- **WHEN** the same `workflow.timer_fired` audit event is dispatched twice
- **THEN** the first dispatch MUST deliver one grant and the second MUST acknowledge as a skip without a second grant

#### Scenario: Authority denial withholds the checkpoint

- **WHEN** normal scheduled composition returns no grant because authority is denied
- **THEN** the consumer MUST classify the event as retryable and MUST NOT advance its checkpoint

### Requirement: Timer dispatch validates owner and dependencies

The timer consumer MUST bind the grant principal to the task owner, reject unknown owners without creating a grant, and resolve every dependency before composition. An unmet dependency MUST atomically mark the task blocked and append a task-blocked audit before acknowledging the timer event.

#### Scenario: Unknown task owner is skipped

- **WHEN** a timer references a task whose owner principal is absent
- **THEN** the consumer MUST acknowledge and skip the event without a worker grant

#### Scenario: Non-owner principal is rejected

- **WHEN** a timer task names an existing principal that is not the configured owner
- **THEN** the consumer MUST acknowledge and skip the event without binding that principal's channel or creating a grant

#### Scenario: Waiting dependency resumes durably

- **WHEN** a timer is blocked by an unfinished dependency and that dependency later transitions to done
- **THEN** durable recovery MUST revalidate all dependencies and owner binding, then dispatch the waiting timer without relying on in-memory retry state

#### Scenario: Unmet dependency surfaces blocked attention

- **WHEN** a task timer fires while any dependency is missing or not done
- **THEN** the task MUST be persisted as blocked with a correlated `task.blocked` audit and the timer MUST be acknowledged without composition

### Requirement: Scheduled grants use applicable least-privilege artifacts

Scheduled timer routes MUST use a capability pack whose `applies_to.lane` is `scheduled_internal` and a workflow declaring that pack. Applicability MUST be checked before composition. The scheduled grant MUST retain owner-channel reply and model/workflow actions needed by the scheduled workflow while denying unrelated proposal authority.

#### Scenario: Scheduled route uses its dedicated pack and workflow

- **WHEN** a deadline or reminder timer is composed
- **THEN** the selected route MUST use the scheduled workflow and pack, the pack MUST apply to the scheduled lane, and `artifact.propose` MUST be denied while owner-channel reply remains available

### Requirement: Correlated task slices are bounded and anchored

The scheduled worker payload MUST be a capped redacted slice anchored on the correlated task, including when a reminder fires before the task deadline. A zero cap MUST return no rows and a cap of one MUST return only the focal task.

#### Scenario: Reminder before due includes its focal task

- **WHEN** a reminder fires for a task whose due time is still in the future
- **THEN** the scheduled payload MUST include that task as the first bounded slice row without exposing dependencies or provenance

