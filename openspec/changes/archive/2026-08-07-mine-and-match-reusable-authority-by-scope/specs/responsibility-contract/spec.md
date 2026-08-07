# responsibility-contract Specification Delta

## MODIFIED Requirements

### Requirement: Trusted action context MUST be resolved by the kernel

The reusable-delegation contract MUST use a kernel-resolved context containing the declared subset of connector implementation/instance, account role/identity, canonical target, bound counterparty, relationship tier, kernel-bound parameters, digests, effect classifications, workflow, and task shape. A shell MUST NOT supply or widen the trusted reviewed scope.

The kernel MUST construct that context at its own boundary — from its own connector, account, target, and counterparty resolution — before consulting any reusable-delegation input, and MUST carry the constructed context rather than a shell-supplied payload into admission. Construction MUST fail closed on a missing implementation descriptor, an unresolvable connector or account, a missing required scope dimension, or an unbound counterparty where the descriptor requires one. The generic shell dispatch path, which receives an opaque payload rather than a digest-bound request, MUST NOT reconstruct a resolved context from that payload.

#### Scenario: Counterparty is unresolved

- **WHEN** a communication action requires counterparty scope but identity resolution yields only a channel identifier
- **THEN** context construction MUST fail before owner review
- **AND** the unresolved identifier MUST NOT become reusable scope

#### Scenario: Kernel constructs the context before consulting reusable input

- **WHEN** an action with a registered implementation descriptor reaches the admission boundary
- **THEN** the kernel MUST construct the resolved context from its own resolution before any reusable-delegation input is consulted
- **AND** no field of that context may originate in shell-supplied request data

#### Scenario: Opaque shell payload cannot become a resolved context

- **WHEN** the generic shell dispatch path receives an opaque payload for an effectful action
- **THEN** it MUST NOT reconstruct a digest-bound resolved context from that payload
- **AND** it MUST continue to fail closed

### Requirement: Reviewed scope matching MUST be protocol-neutral and fail closed

A versioned reviewed action scope MUST be derived from declared generic scope dimensions. Comparison MUST return the exact changed dimensions and MUST NOT branch on protocol names. Missing required dimensions MUST fail closed.

The scope key and the compatibility epoch MUST be distinct digests. The compatibility digest is computed over declaration axes only — descriptor, implementation, executor, resolver, effect destination, required dimensions, egress class, output channels — and therefore MUST NOT be used as the scope key, because it excludes every instance axis and would collide two different accounts, targets, or counterparties into one pattern. A separate reviewed-scope digest MUST be computed over exactly the values named by the descriptor's required scope dimensions, sealed by the same canonical-JSON convention. A reviewed scope MUST persist the individual reviewed value for each required dimension alongside the derived digest, so comparison can name the exact changed dimensions and an owner can narrow one dimension without invalidating the rest. Matching MUST be exact over those sealed values: there MUST be no similarity threshold, no nearest match, and no fuzzy widening of any dimension.

#### Scenario: Synthetic connector context changes

- **WHEN** a non-Gmail synthetic context changes connector instance, account identity, target, or workflow
- **THEN** comparison MUST report those generic dimensions as mismatches
- **AND** no Gmail-specific branch may be required

#### Scenario: Persisted reviewed scope binding is corrupt

- **WHEN** the stored scope dimensions no longer match the stored context-class digest
- **THEN** scope comparison MUST return an invalid-scope outcome
- **AND** reusable execution MUST fail closed

#### Scenario: Compatibility epoch and scope key move independently

- **WHEN** two resolved contexts differ only in an instance axis such as connector instance, account identity, counterparty, canonical target, workflow, or task shape
- **THEN** their compatibility digests MUST be equal
- **AND** their reviewed-scope digests MUST differ

#### Scenario: Declaration change moves only the compatibility epoch

- **WHEN** two resolved contexts differ only in descriptor, implementation, executor, resolver, or policy version
- **THEN** their compatibility digests MUST differ
- **AND** the reviewed-scope digest MUST NOT be relied on to detect that change

### Requirement: Compatibility drift or unavailable resolution MUST require re-review

A changed descriptor, implementation, policy, workflow, reviewed scope, connector availability, or account resolution MUST move the responsibility to a `needs_review` outcome. The kernel MUST NOT silently remap a responsibility to a replacement connector or account.

Drift on any bound epoch MUST restore ordinary owner approval **before** the effect. Because the compatibility and reviewed-scope comparisons run before any budget is reserved and before any effect is dispatched, there MUST NOT exist a window in which a drifted responsibility admits an effect and the drift is only observed afterwards.

#### Scenario: Connector instance disappears

- **WHEN** the reviewed connector/account context can no longer be resolved
- **THEN** compatibility assessment MUST return `needs_review`
- **AND** reusable execution MUST not continue under a guessed successor

#### Scenario: Drift is observed before the effect, not after

- **WHEN** any bound epoch or reviewed dimension of an active responsibility changes
- **THEN** the change MUST be detected before budget is reserved and before the effect is dispatched
- **AND** the action MUST require ordinary owner approval for that request
- **AND** no effect MUST run under the drifted responsibility

### Requirement: Delegation evidence classes MUST remain distinct

Repeated approvals, explicit owner requests, correction/workflow proposals, and manually supplied artifacts MUST be distinct evidence classes. Only repeated approvals MAY support copy claiming that a pattern was observed. Repeated approval evidence MUST bind at least two unique principal-authenticated owner decision events and digest the complete evidence set.

Repeated-approval evidence MUST be grouped by complete resolved context class. Approvals whose reviewed-scope digests differ MUST NOT be aggregated into one pattern, so evidence gathered against one account, counterparty, or canonical target can never support a responsibility scoped to another.

#### Scenario: One or duplicate approval is offered as a pattern

- **WHEN** repeated-approval evidence contains fewer than two unique owner decision events
- **THEN** construction MUST fail
- **AND** the proposal MUST NOT claim an observed pattern

#### Scenario: Repeated approvals belong to a different context class

- **WHEN** repeated-approval evidence carries a context-class digest different from the reviewed scope
- **THEN** owner-review construction MUST fail with a scope-mismatch outcome
- **AND** those approvals MUST NOT support the proposed responsibility

#### Scenario: Non-pattern evidence is rendered

- **WHEN** provenance is constructed from an explicit owner request, correction/workflow proposal, or manually supplied artifact
- **THEN** owner-facing provenance copy MUST be derived from the structured evidence kind
- **AND** it MUST NOT claim that an observed pattern exists

#### Scenario: Approvals across two targets are not one pattern

- **WHEN** owner approvals for one action were gathered against two canonical targets whose reviewed-scope digests differ
- **THEN** they MUST NOT be aggregated into a single repeated-approval evidence set
- **AND** neither target's approvals may be counted toward a pattern scoped to the other
