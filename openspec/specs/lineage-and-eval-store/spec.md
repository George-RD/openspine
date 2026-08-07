# lineage-and-eval-store Specification

## Purpose
TBD - created by archiving change define-lineage-and-eval-store. Update Purpose after archive.
## Requirements
### Requirement: Artifacts MUST carry a generation/lineage model distinct from content version

The kernel MUST provide an `ArtifactLineage` schema type with a derivation
`generation` (u32) and a list of parent references. Lineage MUST be distinct
from the artifact's content `version` (D-028): version tracks edits of one
artifact; generation tracks derivation depth across artifacts. A root
artifact MUST have `generation == 0` and no parents. Artifact rows MUST be
able to carry lineage via a `lineage_json` column on `proposed_artifacts`.

#### Scenario: Root lineage round-trips on an artifact row

Given a proposed-artifact row is inserted with `lineage = Some(root())`
When the row is loaded back through the store API
Then the loaded lineage MUST equal `ArtifactLineage::root()`
And MUST have `generation == 0` and an empty parent list.

#### Scenario: Derived lineage round-trips with parents preserved

Given a proposed-artifact row is inserted with generation 2 and two
`LineageParent` entries
When the row is loaded back through the store API
Then the loaded lineage MUST equal the inserted lineage
And MUST preserve each parent's `kind`, `artifact_id`, and `version`.

#### Scenario: Lineage generation is independent of content version

Given a proposed-artifact row with `version == 1` and
`lineage.generation == 2`
When the row is loaded back
Then `version` MUST remain 1 and `lineage.generation` MUST remain 2
And the two counters MUST NOT be treated as interchangeable.

### Requirement: Unknown lineage MUST NOT be rewritten as root

The `lineage_json` column MUST be nullable. A `NULL` value MUST mean
provenance is unknown (legacy pre-lineage rows) and MUST NOT be silently
rewritten as generation-0 root on migration or load. New inserts that know
their provenance MUST supply an explicit `Some(ArtifactLineage)`.

#### Scenario: A row with no lineage loads as None

Given a proposed-artifact row is inserted with `lineage = None`
When the row is loaded back through the store API
Then the loaded lineage MUST be `None`
And MUST NOT equal `Some(ArtifactLineage::root())`.

### Requirement: Eval verdicts MUST land in an indexed table, not the audit chain

The kernel MUST provide an `eval_verdicts` table (distinct from `audit_log`)
with indexes on artifact identity and on the verdict label. Verdicts MUST
be append-only rows. The table MUST support insert and the following
indexed queries: all verdicts for a `(kind, artifact_id, version)` ordered
by `recorded_at`; all verdicts with a given label; the latest verdict for
a `(kind, artifact_id, version)`.

#### Scenario: Inserted verdicts are returned ordered by recorded_at

Given three eval-verdict rows for the same `(kind, artifact_id, version)`
with increasing `recorded_at` timestamps
When `eval_verdicts_for_artifact` is called
Then the returned list MUST contain exactly those three rows in
ascending `recorded_at` order
And MUST NOT include verdicts for other artifact identities.

#### Scenario: Query by verdict label filters across artifacts

Given verdicts with labels `approved` and `rejected` for different
artifacts
When `eval_verdicts_by_verdict("approved")` is called
Then the returned list MUST contain only rows whose `verdict` equals
`approved`.

#### Scenario: Latest verdict returns the newest for an artifact

Given two eval-verdict rows for the same `(kind, artifact_id, version)`
When `latest_eval_verdict` is called
Then the returned row MUST be the one with the greatest `recorded_at`
And a query for a different version MUST return `None`.

### Requirement: Eval-verdict vocabulary MUST remain open and fitness/evidence optional

The `verdict` column MUST accept any string label — the store MUST NOT
constrain the vocabulary to a closed enum. `fitness` MUST be optional
(`NULL` permitted). `evidence` MUST be optional forward-compatible
metadata. The `evaluator` field is metadata only and MUST NOT confer
authority (D-006). Each row MUST carry `artifact_digest` of the evaluated
bytes (D-011). The store MAY retain `recorded_at` with sub-second
precision, and indexed ordering MUST use its actual temporal value rather
than lossy textual formatting.

Where an evaluator executes concrete cases, its `evidence` MUST record a
structured executed-case ledger: one entry per case carrying the case kind,
the mutated dimension where the case is a scope mutation, the expected
outcome, and the observed outcome. The ledger MUST be sufficient to
reproduce the verdict from stored inputs. An evaluator MUST NOT report a
fitness score in place of a required case outcome, and a passing verdict for
a case-executing evaluator MUST NOT carry an empty ledger.

#### Scenario: An open-vocabulary verdict is accepted

Given an eval-verdict row whose `verdict` label is an arbitrary non-enum
string
When the row is inserted and queried by that label
Then the store MUST return the row
And the store MUST NOT reject the label for being outside a fixed set.

#### Scenario: A case-executing verdict carries a reproducible ledger

Given a passing verdict from an evaluator that executes concrete cases
When the verdict is read back from the store
Then its evidence MUST contain one entry per executed case with the case
kind, expected outcome, and observed outcome
And the mutated dimension MUST be named for every scope-mutation case.

### Requirement: Eval verdicts MUST bind their evaluation epochs and stale when any changes

An eval verdict MUST record the epochs it was computed under: the proposal
digest, the compatibility digest, the reviewed scope digest, the evidence-set
digest where the proposal carries evidence, and the descriptor,
implementation, and policy versions in force. A verdict MUST be treated as
current only while every recorded epoch still equals the live value.
Currency MUST be determined at read time by comparing stored values against
live values, without a background sweeper or a mutating pass over stored
rows. A stale verdict MUST NOT support activation, and MUST force
re-evaluation. Promotion to `review_required` binds the epochs it computed in
the same operation, so it cannot itself observe a stale verdict; the axis a
promotion MUST still enforce is that both witnesses carry the stored
proposal digest.

#### Scenario: A verdict is current while every bound epoch matches

- **GIVEN** a stored passing verdict whose recorded epochs all equal the live values
- **WHEN** currency is evaluated at activation
- **THEN** the verdict MUST be reported as current
- **AND** activation MUST NOT be refused on currency grounds

#### Scenario: A changed compatibility epoch stales the verdict

- **GIVEN** a stored passing verdict
- **WHEN** the action's compatibility digest changes because its descriptor, implementation, executor, resolver, or required scope dimensions changed
- **THEN** the verdict MUST be reported as stale
- **AND** the activation relying on it MUST be refused

#### Scenario: A changed reviewed scope or evidence set stales the verdict

- **GIVEN** a stored passing verdict for a proposal carrying a reviewed scope and an evidence set
- **WHEN** either the reviewed scope digest or the evidence-set digest changes
- **THEN** the verdict MUST be reported as stale
- **AND** re-evaluation MUST be required before the proposal may reach the owner

#### Scenario: Staleness needs no sweeper

- **GIVEN** a stored verdict whose bound epoch has changed and no maintenance pass has run
- **WHEN** currency is evaluated
- **THEN** the verdict MUST already be reported as stale
- **AND** the stored row MUST NOT have been rewritten to make it so

