# workflow-state-machines Specification

## Purpose
TBD - created by archiving change implement-workflow-state-machines. Update Purpose after archive.
## Requirements
### Requirement: Workflow manifests declare a reviewable state machine

A workflow manifest MUST optionally declare an initial state, uniquely identified states, directed transitions, typed deterministic or agentic steps, escalation points, and approval semantics. The manifest MUST remain backward compatible with legacy manifests that contain only the existing descriptive step list and action lists. A valid manifest MUST render its transitions as Mermaid flowchart syntax.

#### Scenario: Legacy manifest remains readable

- **WHEN** a legacy workflow YAML omits `initial_state`, `states`, and `transitions`
- **THEN** it deserializes successfully with empty state-machine collections and retains its descriptive steps

#### Scenario: Typed workflow renders and validates

- **WHEN** a manifest declares states, transitions, typed steps, and an approval action
- **THEN** validation accepts unique references and Mermaid output contains each directed transition

### Requirement: Gateway routing respects declared reasoning tiers

The kernel MUST resolve a workflow step's declared reasoning tier through a static tier-to-provider map before a provider call. Explicit tier overrides MUST select their pre-vetted provider, while absent overrides MUST use the current active provider at resolution time. Production workflow driving and threading this tier through worker execution are deferred to the `worker-runtime` and `seed-workflows` changes; this change's contract is the tested substrate resolver and enforcement boundary.

#### Scenario: High-tier step selects high provider

- **WHEN** a workflow step declares `reasoning_tier: high` and the static map overrides `high` to a high-tier provider
- **THEN** the gateway call is sent through the high-tier provider client rather than the active standard client

#### Scenario: Active provider swap remains visible

- **WHEN** no tier override exists and the active provider id changes after an approved model swap
- **THEN** the next gateway resolution uses the newly supplied active provider id

### Requirement: Approval-semantic transitions are digest-bound and replayable

A transition entering a state marked approval-required MUST validate and atomically persist the declared action request id, action id, payload digest, and target digest in the same durable step that advances the state. A transition leaving a state marked approval-required MUST load the Store-backed `ActionRequest` and `ApprovalRecord`, require the declared action id, and require an approved, unexpired record matching both immutable payload and target digests. Authorization MUST occur before a new workflow ledger step is appended. An authorized approval transition MUST use a typed WorkflowCtx step with a closed non-secret transition outcome, and recovery MUST resume at the next workflow cursor.

#### Scenario: Missing approval blocks departure

- **WHEN** a run reaches an approval-required state and attempts its declared departure without an action request id
- **THEN** the transition is rejected and no new workflow transition is recorded

#### Scenario: Valid digest-bound approval permits departure

- **WHEN** the stored request action matches the state action and the stored approved payload and target digests match an unexpired approval
- **THEN** the typed approval transition completes and the target state becomes current

#### Scenario: Rehydration permits the next transition

- **WHEN** a completed approval transition is rehydrated and the workflow then requests the next declared transition
- **THEN** the prior state and WorkflowCtx cursor are restored and the next transition completes without divergence

