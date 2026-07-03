# artifact-lifecycle Specification

## Purpose
TBD - created by archiving change implement-artifact-lifecycle-slice. Update Purpose after archive.
## Requirements
### Requirement: Proposed artifacts MUST be schema-validated before persistence

`artifact.propose` MUST parse the proposed YAML against the schema for its declared kind before persisting anything, and MUST reject a kind outside `route | agent | workflow | pack | policy`.

#### Scenario: Malformed YAML is rejected

Given a chat proposes an artifact whose YAML does not parse against its declared kind's schema
When `artifact.propose` is dispatched
Then the kernel MUST return a bad-request error
And MUST NOT persist a proposed-artifact row or send an approval button.

#### Scenario: An unknown kind is rejected

Given a chat proposes an artifact with `kind` outside `route | agent | workflow | pack | policy`
When `artifact.propose` is dispatched
Then the kernel MUST return a bad-request error.

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

An artifact activated into the overlay MUST be reloaded into the registry on the next kernel startup, without depending on the in-memory state from the session that activated it.

#### Scenario: Kernel restarts after an activation

Given an artifact was activated into `data/artifacts.d` in a prior kernel run
When the kernel starts up again
Then the artifact MUST be present in the loaded registry
And MUST participate in authority composition exactly as a fixture-loaded artifact would.

