# Responsibility contract

## ADDED Requirements

### Requirement: Reusable delegation MUST validate independent action and implementation declarations

The kernel MUST require a complete catalog-owned action descriptor and a complete concrete implementation descriptor before a reusable-delegation proposal reaches owner review. The implementation MUST identify a resolver and executor with explicit versions. Missing or mismatched declarations MUST fail closed.

#### Scenario: Semantic descriptor exists but executor does not

- **WHEN** `email.create_draft` has reviewed semantics but no reusable implementation descriptor
- **THEN** delegation readiness MUST return a typed missing-implementation error
- **AND** no owner proposal may claim that the reusable effect path is ready

### Requirement: Trusted action context MUST be resolved by the kernel

The reusable-delegation contract MUST use a kernel-resolved context containing the declared subset of connector implementation/instance, account role/identity, canonical target, bound counterparty, relationship tier, kernel-bound parameters, digests, effect classifications, workflow, and task shape. A shell MUST NOT supply or widen the trusted reviewed scope.

#### Scenario: Counterparty is unresolved

- **WHEN** a communication action requires counterparty scope but identity resolution yields only a channel identifier
- **THEN** context construction MUST fail before owner review
- **AND** the unresolved identifier MUST NOT become reusable scope

### Requirement: Reviewed scope matching MUST be protocol-neutral and fail closed

A versioned reviewed action scope MUST be derived from declared generic scope dimensions. Comparison MUST return the exact changed dimensions and MUST NOT branch on protocol names. Missing required dimensions MUST fail closed.

#### Scenario: Synthetic connector context changes

- **WHEN** a non-Gmail synthetic context changes connector instance, account identity, target, or workflow
- **THEN** comparison MUST report those generic dimensions as mismatches
- **AND** no Gmail-specific branch may be required

#### Scenario: Persisted reviewed scope binding is corrupt

- **WHEN** the stored scope dimensions no longer match the stored context-class digest
- **THEN** scope comparison MUST return an invalid-scope outcome
- **AND** reusable execution MUST fail closed

### Requirement: Delegation evidence classes MUST remain distinct

Repeated approvals, explicit owner requests, correction/workflow proposals, and manually supplied artifacts MUST be distinct evidence classes. Only repeated approvals MAY support copy claiming that a pattern was observed. Repeated approval evidence MUST bind at least two unique principal-authenticated owner decision events and digest the complete evidence set.

#### Scenario: One or duplicate approval is offered as a pattern

- **WHEN** repeated-approval evidence contains fewer than two unique owner decision events
- **THEN** construction MUST fail
- **AND** the proposal MUST NOT claim an observed pattern

### Requirement: Owner review MUST be channel-neutral and digest-bound

One semantic owner-review object MUST carry provenance, exact reviewed scope, automatic effects, remaining boundaries, limits, fallback behavior, proposal/compatibility digests, decisions, and lifecycle controls. Transport adapters MUST render and submit against that object rather than inventing channel-specific semantics.

#### Scenario: Review is rendered on another owner surface

- **WHEN** the same review is rendered by Telegram, terminal, or a future authenticated web surface
- **THEN** every surface MUST refer to the same semantic binding digest
- **AND** the contract MUST contain no channel-specific chat or callback fields

#### Scenario: Persisted review bytes are altered

- **WHEN** any semantic review field changes without a new binding digest
- **THEN** binding validation MUST fail
- **AND** the altered review MUST NOT be eligible for an owner decision

### Requirement: Responsibility MUST remain a reference view rather than live authority

A responsibility manifest MUST reference reviewed workflow and standing-rule artifacts and MAY record scope, limits, compatibility, provenance, status, and lifecycle controls. It MUST NOT contain a task grant, action allowlist, capability pack, or direct live executor authority. Every task MUST still receive an ordinary task grant and pass through `gate()`.

#### Scenario: Active responsibility executes another task

- **WHEN** an active responsibility is used for a later matching task
- **THEN** the runtime MUST compose and mint a fresh task grant
- **AND** the responsibility manifest itself MUST NOT authorize the effect

### Requirement: Compatibility drift or unavailable resolution MUST require re-review

A changed descriptor, implementation, policy, workflow, reviewed scope, connector availability, or account resolution MUST move the responsibility to a `needs_review` outcome. The kernel MUST NOT silently remap a responsibility to a replacement connector or account.

#### Scenario: Connector instance disappears

- **WHEN** the reviewed connector/account context can no longer be resolved
- **THEN** compatibility assessment MUST return `needs_review`
- **AND** reusable execution MUST not continue under a guessed successor

### Requirement: Communication dark-window Allow MUST be forbidden

Reusable delegation for communication or connector-write effects MUST reject any policy that permits a dark-window Allow default. Timeout behavior MUST remain deny or require explicit review until a future decision changes this posture.

#### Scenario: Draft action declares bounded Allow

- **WHEN** a communication or owner-account write descriptor declares a bounded Allow dark window
- **THEN** delegation validation MUST fail before owner review
