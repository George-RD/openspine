# artifact-lifecycle Specification

## Purpose
TBD - created by archiving change implement-artifact-lifecycle-slice. Update Purpose after archive.
## Requirements
### Requirement: Proposed artifacts MUST be schema-validated before persistence
`artifact.propose` MUST parse the proposed YAML against the schema for its declared kind before persisting anything, and MUST reject a kind outside `route | agent | workflow | pack | policy | model_swap`.

#### Scenario: Malformed YAML is rejected
- **GIVEN** a chat proposes YAML that does not parse against its declared kind
- **WHEN** `artifact.propose` is dispatched
- **THEN** the kernel MUST return a bad-request error and MUST NOT persist a proposed-artifact row or send an approval button.

#### Scenario: An unknown kind is rejected
- **GIVEN** a chat proposes an artifact with `kind` outside the six supported proposable kinds
- **WHEN** `artifact.propose` is dispatched
- **THEN** the kernel MUST return a bad-request error.

### Requirement: Proposed artifacts MUST follow the lifecycle chain with illegal transitions rejected

A proposed artifact's `state` column MUST only ever move `proposed → validated → review_required → approved → active`, and any attempted transition outside that chain MUST be rejected.

#### Scenario: A proposer cannot pre-activate

Given a chat proposes an artifact whose YAML sets `lifecycle_state: active`
When `artifact.propose` is dispatched
Then the kernel MUST return a bad-request error
And MUST NOT persist the proposal.

### Requirement: Activation MUST require digest-bound owner approval

Activating a proposed artifact MUST require an owner approval whose target digest binds `{kind, artifact_id, version}` and whose payload digest binds the exact YAML bytes reviewed at proposal time; activation MUST re-parse those same bytes rather than accepting any value re-supplied at activation time.

#### Scenario: Owner approves a proposal

Given a proposed artifact is in `review_required`
When the owner taps the approval button bound to its exact YAML and `{kind, artifact_id, version}`
Then the kernel MUST activate the artifact using only the originally-proposed YAML bytes
And MUST insert it into the live registry
And MUST persist it to the on-disk overlay.

#### Scenario: A duplicate proposal for an already-active id and version is rejected

Given an artifact `(kind, artifact_id, version)` is already `active`
When a chat proposes the same `(kind, artifact_id, version)` again
Then the kernel MUST return a bad-request error
And MUST NOT create a second proposed-artifact row.

### Requirement: Artifact id and version MUST be unique across fixtures, overlay, and pending proposals

`artifact.propose` MUST reject a `(kind, artifact_id, version)` that already exists in the live registry (fixture or overlay-loaded) or among pending `proposed_artifacts` rows.

#### Scenario: A pending proposal blocks a duplicate

Given a proposal for `(kind, artifact_id, version)` is already `review_required`
When a chat proposes the same `(kind, artifact_id, version)` again
Then the kernel MUST return a bad-request error naming the id/version collision.

### Requirement: Only active artifacts MUST participate in authority composition

Authority composition MUST only draw on artifacts whose `lifecycle_state` is `active` — a `proposed`, `validated`, `review_required`, `approved`, or `quarantined` artifact MUST NOT be composed into any task grant.

#### Scenario: A quarantined artifact is excluded

Given an artifact is `quarantined`
When authority composition runs
Then the quarantined artifact MUST NOT participate in the composed grant.

### Requirement: Prompt templates MUST NOT be proposable at runtime

`artifact.propose` MUST NOT accept a prompt-template kind — templates remain fixture-only.

#### Scenario: A template proposal is rejected

Given a chat proposes an artifact with `kind: template`
When `artifact.propose` is dispatched
Then the kernel MUST return a bad-request error
And MUST NOT persist anything.

### Requirement: Activated artifacts MUST survive a kernel restart

An artifact activated into the overlay MUST be reloaded into the registry on the next kernel startup only after its durable learned-artifact provenance exists and the startup compatibility pass succeeds. Base fixtures remain upstream-owned; an overlay file without provenance or with dangling learned references MUST be excluded and surfaced for owner review rather than silently becoming effective.

#### Scenario: Kernel restarts after an activation

Given an artifact was activated into `data/artifacts.d` in a prior kernel run
When the kernel starts up again
Then the artifact MUST be present only if its learned provenance row exists and startup compatibility accepts it
And an overlay without provenance or with dangling learned references MUST be excluded and surfaced for owner review.

### Requirement: Authority-bearing proposals require overlay evaluation before approval
Every authority-bearing proposal MUST pass a digest-bound replay and adversarial risk-judge evaluation before reaching `review_required` or exposing an owner approval tap. Both evaluators MUST run against a kernel-assembled evaluation input bound to the exact stored proposal digest; a dimension the kernel cannot resolve MUST produce a typed incomplete-input denial naming that dimension, and MUST NOT be treated as a generic artifact or a pass.

A **reusable-authority proposal** — one that proposes a standing rule admitting an action without per-instance owner approval — MUST additionally satisfy a structural judge. For every such proposal the judge MUST establish that the action exists in the canonical catalog, that it is not catalogued as non-delegable, and that no composed policy denies it. A proposal whose action carries no reusable-delegation descriptor grants blanket authority, so it MUST additionally be an action the catalog declares approval-narrowing — one that narrows an approval requirement rather than admitting an effect; any other action MUST bind a reviewed scope before a standing rule may admit it. Where the action carries a reusable-delegation descriptor, the judge MUST additionally establish that its named implementation has a registered executor and a declared resolver, that the declared delegation contract is eligible, that every action-required reviewed-scope dimension is present and internally consistent, that the effect class and any dark-window configuration are admissible for the action, that budgets and expiry lie within the action's declared bounds, and that the proposal neither equals nor widens the reviewed scope of an active rule held by a different artifact. Its replay pass MUST execute concrete cases against the exact proposed binding and record their outcomes; an evaluation that executed no cases, or whose executed cases do not include both a matching case and a refused changed-context case, MUST be a denial. A standing rule whose action carries no reusable-delegation descriptor has no reviewed scope to vary, so its replay MUST fall to the availability check and MUST NOT be reported as a replay.

Any other authority-bearing proposal MUST still pass structural probes over the canonical catalog and its declared authority lists, and a `model_swap` proposal MUST use kernel-executed golden-set replay as its executed-case evidence. Where an evaluator measures only availability or corpus presence, it MUST be named for what it measures and MUST NOT be reported as a replay. In all paths, both passing verdicts MUST bind the exact stored proposal digest and persist in the eval-verdict store.

#### Scenario: Proposal with two passing evaluations reaches approval
- **GIVEN** a validated authority-bearing proposal whose evaluation input resolves completely
- **WHEN** both evaluators pass for the stored proposal digest
- **THEN** both verdicts are persisted and the proposal transitions to `review_required`
- **AND** the owner approval summary includes the evaluation evidence

#### Scenario: Reusable-authority proposal whose action has no registered executor is denied
- **GIVEN** a standing-rule proposal naming an implementation with no registered executor
- **WHEN** the overlay evaluation gate runs
- **THEN** the gate MUST deny with an executor-readiness reason
- **AND** the proposal MUST NOT reach `review_required`
- **AND** no verdict recording a pass MUST be persisted

#### Scenario: Proposal with an incomplete evaluation input is denied by dimension
- **GIVEN** a standing-rule proposal whose reviewed-scope binding omits a dimension the action requires, or whose stored scope digest disagrees with its stored values
- **WHEN** the kernel assembles the evaluation input
- **THEN** assembly MUST fail with an incomplete-input denial naming the missing or inconsistent dimension
- **AND** the proposal MUST NOT be evaluated as a generic artifact

#### Scenario: Policy-denied or out-of-bounds reusable-authority proposal is refused
- **GIVEN** a standing-rule proposal for an action the composed policy denies, or whose budgets or expiry fall outside the action's declared bounds
- **WHEN** the structural judge runs
- **THEN** the judge MUST deny naming the failing axis
- **AND** the proposal MUST NOT reach `review_required`

#### Scenario: Ambiguously overlapping or widening proposal is refused
- **GIVEN** an active rule for the action and a standing-rule proposal whose reviewed scope equals or widens that rule's reviewed scope
- **WHEN** the structural judge runs
- **THEN** the judge MUST deny as an ambiguous or widening overlap
- **AND** a proposal whose reviewed scope is disjoint from every active rule MUST NOT be denied on that axis

#### Scenario: An evaluation that executed no cases cannot pass as replay
- **GIVEN** a reusable-authority proposal whose replay produced an empty executed-case ledger, or a ledger with no refused changed-context case
- **WHEN** the gate evaluates the replay result
- **THEN** the gate MUST deny
- **AND** the denial MUST NOT be recorded or rendered as a replay pass

#### Scenario: Proposal without captured owner history is denied
- **GIVEN** an authority-bearing proposal other than `model_swap` whose replay falls to the availability check, with no provenance-filtered owner-control history
- **WHEN** the overlay evaluation gate runs
- **THEN** the proposal does not reach `review_required`
- **AND** no owner approval button is sent

#### Scenario: A non-delegable action cannot carry a standing rule
- **GIVEN** a standing-rule proposal for an action the catalog marks non-delegable
- **WHEN** the structural judge runs
- **THEN** the judge MUST deny naming delegability
- **AND** the proposal MUST NOT reach `review_required`

#### Scenario: An uncatalogued action cannot carry a standing rule
- **GIVEN** a standing-rule proposal naming an action id that is not in the canonical catalog
- **WHEN** the structural judge runs
- **THEN** the judge MUST deny as an unknown action
- **AND** the proposal MUST NOT reach `review_required`

#### Scenario: An effectful action cannot carry an unscoped standing rule
- **GIVEN** a standing-rule proposal that binds no reviewed scope, for a catalogued action the catalog does not declare approval-narrowing
- **WHEN** the structural judge runs
- **THEN** the judge MUST deny, requiring a reviewed scope
- **AND** a proposal for an action the catalog does declare approval-narrowing MUST NOT be denied on that axis

#### Scenario: Generic lifecycle bypass is rejected
- **GIVEN** code attempts a direct `validated` to `review_required` mutation or inserts a proposal already in `review_required`
- **WHEN** the store boundary handles the operation
- **THEN** it rejects the operation because only the digest-bound evaluation promotion can expose approval

#### Scenario: Model swap with two passing evaluations reaches approval
- **GIVEN** a validated model_swap proposal has kernel-generated golden-set evidence
- **WHEN** replay and risk-judge evaluators pass for the stored YAML digest
- **THEN** both verdicts MUST be persisted and the proposal MUST transition to `review_required`
- **AND** the owner approval summary MUST include role, target provider, and bounded observed case evidence.

#### Scenario: Missing model-swap evaluation blocks approval
- **GIVEN** either model-swap evaluator is unavailable or fails
- **WHEN** a model_swap proposal is dispatched
- **THEN** the proposal MUST remain outside the approval surface.

#### Scenario: Model swap lifecycle bypass is rejected
- **GIVEN** code attempts to insert a model_swap proposal already in `review_required` or directly mutate it into `review_required`
- **WHEN** the store boundary handles the operation
- **THEN** it rejects the operation because only the digest-bound replay and risk-judge promotion can expose approval

### Requirement: Learned artifacts MUST carry durable exchange provenance

Every activated learned overlay artifact MUST have a non-null producing event identifier and encrypted exchange digest before it becomes visible in the registry.

#### Scenario: Activation without provenance is rejected

Given an artifact activation has no source event or exchange digest
When the kernel attempts to record the learned artifact
Then the store MUST reject the write
And the artifact MUST NOT become effective.

### Requirement: Compatibility MUST fail closed for dangling learned references

After a base update, the kernel MUST validate learned route and workflow references against the merged registry to a fixed point. A dangling reference MUST create a pending re-confirmation and MUST exclude the learned artifact and its learned dependents from effective authority composition.

#### Scenario: Base update orphans a learned route

Given an active learned route references a base agent
When an update removes that agent
Then the compatibility pass MUST record re-confirmation for the route
And the route MUST be absent from the effective registry until reviewed.

### Requirement: Upstream nomination MUST be explicit and opt-in

A learned overlay artifact MAY be nominated as an upstream candidate only through a normal digest-bound review whose request explicitly asserts that the content is depersonalized. Nomination MUST NOT change the artifact namespace automatically.

#### Scenario: Personal artifact cannot be nominated implicitly

Given a learned overlay artifact
When a nomination request omits or falsifies the depersonalized opt-in
Then the kernel MUST reject the request
And the artifact MUST remain an overlay artifact.

### Requirement: Overlay version cutover MUST be highest-only and monotonic

For every artifact kind, proposal and activation MUST reject exact duplicates and lower versions. Activating a higher version MUST atomically replace the prior live version, append an `artifact.superseded` audit, and expose the same highest-only registry after restart.

#### Scenario: Highest version wins across two boots

Given overlay versions v1 and v2 for one identity
When v2 activates and the kernel boots twice
Then only v2 participates in authority on both boots
And the prior v1 is recorded as superseded.

### Requirement: Legacy overlay migration is discovery/quarantine only

`LegacyMigration` is a quarantine placeholder only. When an overlay file has no learned-artifact row, startup MUST synthesize `LegacyMigration` provenance using the actual discovered path and bytes and require digest-bound owner reconfirmation before exposure. A successful owner tap MUST mint a fresh digest-bound proposal and establish `ProducedBy` exchange provenance (producing event id + encrypted exchange digest) BEFORE the artifact becomes visible; the kernel MUST NOT activate an artifact whose effective provenance remains `LegacyMigration`.

#### Scenario: Legacy tap establishes ProducedBy before visibility

Given a quarantined overlay with `LegacyMigration` provenance pending owner review
When the owner reconfirms the exact reviewed bytes
Then the accepted row carries fresh `ProducedBy` exchange provenance and a ReconfirmAnchor
And a fresh proposal is minted and advanced to `Active`
And the artifact is visible only under `ProducedBy` provenance.

#### Scenario: Non-canonical legacy filename survives review

Given a valid learned overlay stored at a non-canonical YAML filename
When startup quarantines and the owner reconfirms it
Then the reviewed bytes are read from that actual source path and the artifact is restored.

### Requirement: Base and overlay namespace collisions MUST refuse replacement

An overlay identity colliding with a base identity MUST remain excluded from authority and owner reconfirmation MUST refuse to replace the base artifact. The base artifact MUST survive both the collision boot and the next boot.

#### Scenario: Collision cannot delete base authority

Given base and overlay artifacts share a kind and id
When the kernel boots twice
Then the overlay is pending owner review and the base remains active on both boots.

### Requirement: Owner acceptance MUST bind to the reviewed base epoch

OwnerAccepted MUST persist the sorted active-base kind/id/version/content-reference epoch and reconfirm provenance, and MUST record a `ReconfirmAnchor` (request id, grant event id, reviewed bytes ref) for every successful reconfirmation regardless of provenance kind. When the base epoch changes, the kernel MUST revalidate the overlay's typed dependencies: compatible overlays refresh the stored epoch without prompting, while newly dangling references are excluded and receive a new pending review. An unchanged restart MUST retain acceptance.

#### Scenario: Base epoch change only prompts newly dangling overlays

Given an owner-accepted overlay and a recorded base compatibility epoch
When an unrelated active base artifact changes
Then the overlay remains owner-accepted and its epoch is refreshed
but when a referenced base artifact changes incompatibly
Then the overlay is excluded and a new digest-bound reconfirmation is pending.

### Requirement: Owner reconfirmation MUST commit atomically before publication

The action request consumption, learned-row `OwnerAccepted` update, matching proposal `Approved -> Active` transition, and acceptance/activation/superseded audits MUST all commit in a single transaction BEFORE the artifact is published to the live registry. A failed or rolled-back commit MUST leave the registry unchanged, the action request retryable, and MUST NOT emit a success audit or owner notification. A concurrent or duplicate tap that loses the consume race MUST publish nothing.

#### Scenario: Failed commit is retryable and publishes nothing

Given a pending overlay reconfirmation
When the durable transaction fails during the owner tap
Then the live registry is unchanged, no success audit is emitted, and the owner may tap again to retry.

### Requirement: Personality seed artifacts MUST load as overlay learned artifacts with provenance

The Donna×Leo personality seed (AD-080) MUST ship as pre-populated, learnable
`persona` overlay artifacts — never as base/kernel-baked fixtures. Each seed
artifact MUST load into the artifact registry and MUST carry a non-null
`ProducedBy` exchange provenance (D-077) so it enters the effective registry
through the same overlay machinery as any owner-learned artifact.

#### Scenario: Seed elements load into the registry with ProducedBy provenance

- **GIVEN** a kernel boot with an empty `learned_artifacts` store
- **WHEN** the personality seed bootstrap runs and the overlay is loaded
- **THEN** the registry contains the eight AD-080 elements plus the AD-082
  digest/brief default as `persona` artifacts
- **AND** each loaded persona row has `namespace = overlay` and
  `Provenance::ProducedBy` with a non-null source event and encrypted exchange digest

#### Scenario: Seed survives a kernel restart

- **GIVEN** the personality seed has been written to `data/artifacts.d/personas`
  and recorded in `learned_artifacts`
- **WHEN** the kernel boots again
- **THEN** the persona artifacts are present in the registry on the second boot
- **AND** no duplicate `learned_artifacts` rows are created for the same
  `(kind, artifact_id, version)`

### Requirement: Personality seed MUST NOT be kernel-baked base fixtures

Persona seed artifacts MUST live only in the overlay namespace. The loader MUST
NOT source them from the base fixture directory, and the kernel MUST NOT treat
a seeded persona as a base artifact during the base/overlay compatibility pass.

#### Scenario: Seed is excluded from base identity and compatibility

- **GIVEN** a seeded persona artifact in the overlay
- **WHEN** the compatibility pass and the base-identity collision check run
- **THEN** the persona is present only in the overlay namespace
- **AND** it is never counted as a base identity or flagged as a base collision

### Requirement: Personality seed seeding MUST be idempotent across boots

The seed bootstrap MUST treat a persona as converged only when its
`learned_artifacts` row exists and its on-disk YAML is present with the row's
recorded digest. It MUST durably write and fsync each element's YAML before
recording its provenance row, and MUST converge repeated or partially-completed
boots without duplicate rows or dangling provenance.

#### Scenario: A second boot seeds nothing new

- **GIVEN** the seed has already written all nine persona elements
- **WHEN** the seed bootstrap runs again on a fresh boot
- **THEN** no new `learned_artifacts` rows are inserted
- **AND** the overlay directory still contains exactly the nine seeded files

#### Scenario: A crash between file write and provenance row self-heals

- **GIVEN** the seed wrote a persona YAML file but the kernel crashed before
  recording its provenance row
- **WHEN** the kernel boots again and the seed bootstrap runs
- **THEN** the missing element is written and recorded, converging to the full set

### Requirement: Every AD-081/AD-083 anti-pattern MUST have an eval probe

The eval harness MUST define a deterministic probe for each personality
anti-pattern: the seven from AD-081 (deferential double-asking, sycophancy,
over-explaining, nagging, presumptuous anticipation, need-to-know failure,
apology theater) and the three AD-083 additions (faked intimacy, info-dump
without synthesis, self-promotional visibility).

#### Scenario: All ten anti-patterns are represented

- **GIVEN** the eval-harness probe registry
- **WHEN** it is enumerated
- **THEN** it contains exactly one probe for each of the ten AD-081/AD-083
  anti-patterns

### Requirement: Anti-pattern probes MUST fail on violating output and pass on clean output

A personality anti-pattern probe MUST return a violation when the
output-under-test exhibits that anti-pattern, and MUST return no violation on
clean output, so the eval harness can fail any violating sample.

#### Scenario: A violating sample trips its probe

- **GIVEN** an output string that exhibits one AD-081/AD-083 anti-pattern
- **WHEN** the corresponding probe runs against it
- **THEN** the probe returns a violation naming that anti-pattern

#### Scenario: Clean output trips no probe

- **GIVEN** an output string with none of the AD-081/AD-083 anti-patterns
- **WHEN** all probes run against it
- **THEN** no probe returns a violation

#### Scenario: A committed row with a missing or corrupt file self-heals

- **GIVEN** a canonical persona `learned_artifacts` row exists but its YAML is
  missing or its bytes no longer match the canonical seed digest
- **WHEN** the kernel boots and the personality seed bootstrap runs
- **THEN** the seed republishes the canonical seed content when the row has
  valid, resolvable `ProducedBy` provenance
- **AND** it leaves that valid provenance row untouched
- **AND** a row with a dangling event, unbound exchange, or foreign digest is
  quarantined and reseeded rather than blocking the canonical persona forever

#### Scenario: Learned row and seeded receipt are atomic

- **GIVEN** the durable YAML has been published and the `personality_seed.seeded`
  audit append fails
- **WHEN** the seed transaction rolls back
- **THEN** neither the persona `learned_artifacts` row nor its `personality_seed.seeded`
  receipt remains committed
- **AND** a later boot can retry the element without a permanently missing receipt

### Requirement: Persona artifacts MUST never enter kernel authority

`persona` MUST remain absent from the proposable-kind table and MUST NOT have a
`ParsedProposal` representation. The propose and upstream-nomination paths MUST
reject `persona`, and no persona artifact may enter grant, gate, approval, or
activation authority.

#### Scenario: Persona proposal and nomination are rejected

- **GIVEN** a caller submits a `persona` proposal or upstream nomination
- **WHEN** the kernel validates the artifact kind
- **THEN** it returns a structured bad request
- **AND** it records no proposed artifact, approval request, grant, or activation

### Requirement: Seed guidance MUST keep negative constraints in eval probes

Seeded `PersonaElement.guidance` MUST be positive desired behavior only. AD-054
anti-pattern names, citations, and negative/meta-guidance MUST remain exclusively
in deterministic eval probes, never in shipped persona guidance.

#### Scenario: Seed guidance contains no probe-only constraint text

- **GIVEN** the nine seeded persona elements
- **WHEN** their guidance is loaded
- **THEN** no guidance contains AD-081/AD-083 anti-pattern names or citations
- **AND** `run_probes` remains the sole executable negative-constraint boundary

### Requirement: Digest/brief format MUST remain a learnable default

The AD-082 digest/brief shape MUST ship as an overridable persona overlay
default, not as an enforced schema field, immutable prompt authority, or
kernel grant/gate rule. This change MUST NOT claim an owner-correction or
miner/proposal route that it does not implement; `implement-reflection-miner`
owns that later correction→miner→proposal route while this change preserves
the default-only boundary.

#### Scenario: Digest default is presentation guidance only

- **GIVEN** the `digest_brief_default` persona element
- **WHEN** the registry loads it
- **THEN** it is a normal learnable `persona` overlay artifact whose guidance
  can be replaced by a future correction lane
- **AND** no schema validation, proposal authority, grant, or gate enforces its
  presentation text as an immutable format

### Requirement: Overlay evaluation MUST NOT grant or activate authority
A passing overlay evaluation MUST move a proposal no further than `review_required`. Evaluation MUST NOT mint a task grant, activate or supersede a standing rule, reserve or commit budget, schedule a dark-window pending row, or execute any connector effect. Replay MUST be a pure decision over in-memory candidate bindings.

#### Scenario: A passing evaluation changes no runtime authority
- **WHEN** a reusable-authority proposal passes both evaluators
- **THEN** the proposal MUST be in `review_required`
- **AND** no rule MUST be active for its action as a result
- **AND** no standing-rule usage row, dark-window pending row, or task grant MUST have been created

#### Scenario: Replay causes no external effect
- **WHEN** replay executes its case set for a proposal whose action is a connector write
- **THEN** no connector effect MUST be dispatched
- **AND** no pending-write fence row MUST be created

### Requirement: Gate summary copy MUST state only what executed cases prove
The owner-facing overlay gate summary MUST be derived from the stored verdicts and their recorded evidence, not authored as free text. It MUST report what actually ran, and MUST NOT describe an evaluation as a replay of prior or synthetic cases unless concrete cases were executed and recorded.

#### Scenario: Summary reports executed case counts
- **GIVEN** a reusable-authority proposal whose replay executed evidence-derived matching cases and changed-context cases
- **WHEN** the gate renders the owner summary
- **THEN** the summary MUST state the number and kinds of cases executed
- **AND** every claim in the summary MUST be derivable from the stored verdicts

#### Scenario: No replay claim without executed cases
- **GIVEN** an evaluation whose evidence records no executed cases
- **WHEN** the gate renders any owner-facing copy
- **THEN** that copy MUST NOT assert that prior or synthetic cases were replayed

