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
Every authority-bearing proposal MUST pass a digest-bound replay and adversarial risk-judge evaluation before reaching `review_required` or exposing an owner approval tap. Every authority-bearing proposal other than `model_swap` MUST replay against provenance-filtered captured owner-control history; if that history or either evaluator is unavailable, the proposal MUST remain outside the approval surface. A `model_swap` proposal MUST instead use kernel-executed golden-set replay as its captured replay evidence. In both paths, both passing verdicts MUST bind the exact stored proposal digest and persist in the eval-verdict store.

#### Scenario: Proposal with two passing evaluations reaches approval
- **GIVEN** a validated route, agent, workflow, pack, or policy proposal
- **AND** captured owner-control history is available
- **WHEN** replay and risk-judge evaluators pass for the stored YAML digest
- **THEN** both verdicts are persisted and the proposal transitions to `review_required`
- **AND** the owner approval summary includes evaluation evidence

#### Scenario: Proposal without captured owner history is denied
- **GIVEN** an authority-bearing proposal other than `model_swap` with no provenance-filtered owner-control history
- **WHEN** the overlay evaluation gate runs
- **THEN** the proposal does not reach `review_required`
- **AND** no owner approval button is sent

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

