## MODIFIED Requirements

### Requirement: Reusable delegation MUST validate independent action and implementation declarations


The kernel MUST require a complete catalog-owned action descriptor and a complete concrete implementation descriptor before a reusable-delegation proposal reaches owner review. The implementation MUST identify a resolver and executor with explicit versions. Executor readiness MUST additionally require that the descriptor's `executor_id` is registered in the kernel-owned effect-executor registry for the action. A descriptor alone MUST NOT prove that the action is runnable. Missing or mismatched action, resolver, implementation, or executor declarations MUST fail closed.

A second communication shape MUST become delegable only by declaring the same two descriptors — a catalog-owned `ActionDescriptor` in `delegation_descriptors()` and a concrete `ActionImplementationDescriptor` in `implementation_descriptors()`, each with its own resolver id, executor id, and explicit versions — plus an explicit egress declaration. A `None`/`None` egress declaration is a deliberate non-egress classification and MUST be written literally; an action absent from the egress table MUST fail closed rather than default. Declaring a shape MUST NOT self-authorize its delegation: the catalog's non-delegable set, the counterparty-facing set, and the dark-window eligibility allowlist continue to decide that, and a shape whose declared destination is a communication or connector write MUST carry `DarkWindowPolicy::Prohibited`.

#### Scenario: Semantic descriptor exists but executor does not

- **WHEN** `email.create_draft` has reviewed semantics but no reusable implementation descriptor
- **THEN** delegation readiness MUST return a typed missing-implementation error
- **AND** no owner proposal may claim that the reusable effect path is ready
- **AND** the typed error MUST be `MissingImplementationDescriptor`

#### Scenario: Descriptor exists but executor is not registered

- **WHEN** `email.create_draft` has reviewed semantics and a complete implementation descriptor but its declared `gmail.create_draft` executor is not registered
- **THEN** execution readiness MUST return false
- **AND** no owner proposal may claim that the reusable effect path is ready
- **AND** dispatch MUST NOT return a successful stub.

Test: `is_execution_backed_requires_descriptor_and_registered_executor`

#### Scenario: Descriptor and registered executor establish readiness

- **WHEN** `email.create_draft` has its action-keyed D-146 descriptor and the declared `gmail.create_draft` executor is registered
- **THEN** execution readiness MUST return true
- **AND** the readiness result MUST identify the descriptor-plus-registry conjunction rather than a separate effect-class enum.

#### Scenario: A second shape declares its own descriptor and implementation pair

- **WHEN** a second communication shape is added with its own action descriptor, implementation descriptor, resolver, executor, and explicit egress declaration
- **THEN** delegation readiness for that shape MUST be established by the same descriptor-plus-registered-executor conjunction as the first shape
- **AND** no generic readiness, matching, evaluation, review, or lifecycle code may branch on which shape it is

#### Scenario: A declared shape is not thereby authorized

- **WHEN** a newly declared shape's destination is a shared workspace or other communication or connector write
- **THEN** its declared dark-window policy MUST be `Prohibited`
- **AND** a dark-window `Allow` default naming it MUST be refused at activation

#### Scenario: A shape declared without an egress row fails closed

- **WHEN** a shape is added to the catalog with no literal egress declaration
- **THEN** the gate MUST fail closed for that action
- **AND** the omission MUST NOT be read as a non-egress classification


### Requirement: Reviewed scope matching MUST be protocol-neutral and fail closed


A versioned reviewed action scope MUST be derived from declared generic scope dimensions. Comparison MUST return the exact changed dimensions and MUST NOT branch on protocol names. Missing required dimensions MUST fail closed.

The scope key and the compatibility epoch MUST be distinct digests. The compatibility digest is computed over declaration axes only — descriptor, implementation, executor, resolver, effect destination, required dimensions, egress class, output channels — and therefore MUST NOT be used as the scope key, because it excludes every instance axis and would collide two different accounts, targets, or counterparties into one pattern. A separate reviewed-scope digest MUST be computed over exactly the values named by the descriptor's required scope dimensions, sealed by the same canonical-JSON convention. A reviewed scope MUST persist the individual reviewed value for each required dimension alongside the derived digest, so comparison can name the exact changed dimensions and an owner can narrow one dimension without invalidating the rest. Matching MUST be exact over those sealed values: there MUST be no similarity threshold, no nearest match, and no fuzzy widening of any dimension.

Visibility semantics of a communication shape — whether an effect is addressed to a shared channel or to a direct message, which channel it lands in, and which participants can see it — MUST be expressed through the existing generic scope dimensions and MUST NOT introduce a protocol-specific dimension variant. `EffectDestination` binds the workspace-versus-direct-message distinction, `OutputChannel` binds the channel, and `BoundParameters` binds the kernel-resolved participant or member set. Two contexts that differ only in visibility MUST therefore differ in reviewed-scope digest while sharing a compatibility epoch, exactly as two accounts or targets do. Cross-shape, cross-account, cross-target, and cross-visibility confusion MUST all be reported as generic dimension mismatches, and comparison MUST remain free of any branch naming a protocol.

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

#### Scenario: A channel-visible effect and a direct message are different reviewed scopes

- **WHEN** two resolved contexts differ only in whether the effect is addressed to a shared channel or to a direct message
- **THEN** their reviewed-scope digests MUST differ on `EffectDestination`
- **AND** their compatibility digests MUST be equal
- **AND** a rule reviewed for one MUST NOT admit the other

#### Scenario: A widened participant set stops a rule matching

- **WHEN** the kernel-resolved participant set bound in `BoundParameters` gains a member after review
- **THEN** comparison MUST report `BoundParameters` as the changed dimension
- **AND** admission MUST return to ordinary owner approval before any effect

#### Scenario: Two shapes with equal instance axes are still distinct

- **WHEN** two resolved contexts name different communication shapes but agree on every instance value the descriptors share
- **THEN** their compatibility digests MUST differ because the descriptor and implementation axes differ
- **AND** neither shape's reviewed rule may admit the other


## ADDED Requirements

### Requirement: A second communication shape MUST complete the delegation path without engine changes

A second communication shape MUST complete the whole delegation path — propose, evaluate, owner review, activate, scoped reuse, one real effect, responsibility receipt, and lifecycle controls — through the same descriptor, resolver, executor, reviewed scope, evidence, evaluation, owner-review, lifecycle, receipt, and fallback contracts as the first shape. Adding the shape MUST supply reviewed adapters and deterministic fixtures only; it MUST NOT add a protocol branch to generic matching, evaluation, review, receipt, or lifecycle code, and it MUST NOT introduce a second authority object.

The shape MAY be served by a deterministic in-repo test connector rather than an external service, provided that connector models workspace, direct-message, and channel visibility semantics rather than renaming the first shape's fields. It MUST introduce no new external credential, OAuth flow, or network dependency.

Every fallback the first shape proves MUST hold for the second: erased counterparty, bound-context drift, exhausted quota or rate, pause, expiry, revocation, an unresolved counterparty, evaluation staleness, and a fenced retry MUST each return the action to ordinary owner approval or deny it before any effect.

#### Scenario: The second shape completes the owner path

- **WHEN** repeated owner approvals for the second shape produce a proposal that the owner reviews and approves
- **THEN** activation MUST use the ordinary artifact lifecycle with a fresh owner-minted activation grant
- **AND** the next matching request MUST produce exactly one real effect through that shape's registered executor with a responsibility receipt rather than another approval prompt

#### Scenario: The second shape's fallbacks all hold before effect

- **WHEN** any of erased counterparty, bound-context drift, exhausted quota or rate, pause, expiry, revocation, unresolved counterparty, or stale evaluation applies to a second-shape request
- **THEN** admission MUST return ordinary owner approval or deny
- **AND** no effect may reach the connector
- **AND** no reviewed budget may be consumed by the refused attempt

#### Scenario: Adding the shape changes no generic engine code

- **WHEN** the second shape is added
- **THEN** reviewed-scope comparison, evidence construction, evaluation, owner review, lifecycle controls, and receipt assembly MUST be unchanged apart from data declarations and fixtures
- **AND** no generic function may accept a shape or protocol discriminator
