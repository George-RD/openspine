# nerve-subscribers Specification

## Purpose
TBD - created by archiving change implement-nerve-subscribers. Update Purpose after archive.
## Requirements
### Requirement: Nerve declaration schema
The system MUST provide a versioned nerve declaration that binds a subscription filter, measure, speak threshold, hard budget, model tier, and data scope to one of the declared nerve types: advisor, injector, screener, miner, or meta-cognition.

#### Scenario: Declared types and fields round-trip
- **WHEN** a nerve declaration of each declared type is serialized and deserialized
- **THEN** all fields (subscription filter, measure, speak threshold, budget, model tier, scope) survive round-trip unchanged

#### Scenario: Complete declaration shapes cover every type
- **WHEN** complete declarations are constructed for advisor, injector, screener, miner, and meta-cognition with non-empty filter, measure, threshold, budget/window, model tier, and scope fields
- **THEN** each declaration round-trips unchanged and its type/measure pairing is validated before registration

#### Scenario: Scope is bounded by the advisee
- **WHEN** a declaration sets its scope data classes or scopes wider than its advisee's scope
- **THEN** the declaration is invalid and MUST NOT be considered registrable

### Requirement: Registration validates advisee scope
The system MUST reject registration of any nerve whose data scope exceeds the scope of the agent it advises. A nerve declared with broader data access than its advisee is UNREGISTRABLE.

#### Scenario: Wider-scope nerve is unregistrable
- **GIVEN** an advisee whose scope permits classes ["email", "memory"]
- **WHEN** a nerve declares scope classes ["email", "memory", "secret"]
- **THEN** registration fails with a scope-exceeds-advisee error and no registration row is written

#### Scenario: Equal-or-narrower scope registers
- **GIVEN** an advisee whose scope permits classes ["email", "memory"]
- **WHEN** a nerve declares scope classes ["email"]
- **THEN** registration succeeds and the declaration is retrievable

#### Scenario: Model tier must not exceed advisee tier
- **GIVEN** an advisee whose maximum permitted model tier is Standard
- **WHEN** a nerve declares model tier Strong
- **THEN** registration fails with a tier-exceeds-advisee error

### Requirement: Advisor interjections are structured legibility objections
The system MUST represent an advisor interjection as a structured objection carrying a concern class and cited clause, plus provenance and gate-visibility. Advisor output MUST NOT carry replacement answers or caller-authored rewrites.

#### Scenario: Advisor objection carries structure, not an answer
- **WHEN** an advisor nerve emits an interjection
- **THEN** the interjection includes concern class and cited clause
- **AND** the interjection has no answer or rewrite field

#### Scenario: Cross-scope advisor hint is gate-visible
- **WHEN** an advisor interjection concerns data outside the nerve's own scope
- **THEN** the interjection is marked gate_visible and carries provenance and cited clause as a structured message, never ambient context

### Requirement: Proactivity is a budgeted lane
The system MUST treat interjections as a budgeted lane: each admitted interjection consumes one hard budget unit, provenance is required, and a class retired after five ignored user reactions MUST NOT emit further interjections.

#### Scenario: Admitted interjection consumes budget
- **GIVEN** a registered nerve with a hard budget of one suggestion per window
- **WHEN** a threshold-meeting interjection is admitted
- **THEN** one budget unit is consumed and a second admission in the same window is denied

#### Scenario: Ignored reactions retire a noisy class
- **GIVEN** a registered nerve with a class that the user has ignored four times
- **WHEN** the user ignores the class a fifth time
- **THEN** the class is retired and subsequent interjections of that class are rejected

#### Scenario: Engaged or annoyed reactions do not retire
- **GIVEN** a registered nerve class ignored fewer than five times
- **WHEN** the user reacts Engaged or Annoyed
- **THEN** the ignored counter does not advance and the class remains eligible

#### Scenario: Reaction signals are durable recorded-only hooks
- **GIVEN** store-issued interjections for one class
- **WHEN** the user reacts Engaged, Ignored, or Annoyed
- **THEN** the corresponding durable counters are recorded, only Ignored advances retirement, and repeating a reaction for the same interjection is idempotent

### Requirement: Interjections ride the archived event-bus substrate
The system MUST bind a nerve's subscription filter to the existing `EventSubscriptionFilter` type and MUST NOT introduce a second event store or live broker. Registration metadata and decay counters MUST contain no interjection text or private payload (D-012).

#### Scenario: Filter reuses event-bus type
- **WHEN** a nerve declaration specifies its subscription filter
- **THEN** the filter is the existing `EventSubscriptionFilter` value used by the audit-ledger bus

#### Scenario: Registration binds the exact persisted filter without plaintext payload
- **WHEN** a nerve is registered and reactions are recorded
- **THEN** the namespaced nerve checkpoint persists the exact `EventSubscriptionFilter`
- **AND** registration/budget/decay/issuance/reaction rows contain only policy metadata, opaque class digests, counters, and ids—not interjection bodies, secrets, or private payload text

#### Scenario: Registered nerves replay through typed handlers
- **WHEN** the kernel replays the audit ledger for registered nerves
- **THEN** each declaration's exact persisted filter selects its events, the typed handler receives the declaration and event, and the checkpoint advances only after the handler succeeds

### Requirement: Manifest limits and production screener dispatch are kernel-owned
The kernel MUST derive registrable advisee limits from active `AgentManifest` values as a full snapshot, conservatively removing any allowed class that overlaps a denied class. Owner-control ingestion MUST atomically append `event.received` and, when a known manipulation marker is found, a structured `manipulation_signal.detected` event bound to `owner_control`; the typed screener handler MUST consume only explicitly registered screener declarations.

#### Scenario: Active manifest snapshot narrows authority
- **WHEN** startup seeds limits from the active manifest registry
- **THEN** retired or absent manifests have no remaining limits row
- **AND** an allowed class overlapping a denied exact class, parent, or dot-child is excluded

#### Scenario: Owner-control marker screening is atomic and structured
- **WHEN** owner-control ingestion contains a known manipulation marker
- **THEN** `event.received` and `manipulation_signal.detected` commit in one transaction
- **AND** the signal contains only the marker and kernel-bound `owner_control` aggregate, never plaintext

#### Scenario: Other lanes are not attributed to owner-control screening
- **WHEN** a non-owner-control lane emits `event.received`
- **THEN** it uses the ordinary audit path and does not emit `manipulation_signal.detected`

#### Scenario: Unsupported dispatcher types retain their checkpoint
- **WHEN** the screener dispatcher replays registered nerves
- **THEN** only screener declarations build consumers and advance checkpoints
- **AND** generic type-agnostic replay remains available for other typed handlers

#### Scenario: Current narrowing revokes stale registrations
- **WHEN** replay sees a registered nerve whose declaration exceeds the current manifest-derived advisee limits
- **THEN** the registration, budget, decay, and consumer checkpoint are revoked before delivery

#### Scenario: Gate-visible admission is durably delivered
- **WHEN** a screener interjection is admitted and budget-debited
- **THEN** a metadata-only delivery row containing its id and opaque class digest is committed in the same transaction
- **AND** the owner notification drain can deliver and acknowledge that row

