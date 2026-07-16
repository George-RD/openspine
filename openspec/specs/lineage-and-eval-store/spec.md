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

#### Scenario: An open-vocabulary verdict is accepted

Given an eval-verdict row whose `verdict` label is an arbitrary non-enum
string
When the row is inserted and queried by that label
Then the store MUST return the row
And the store MUST NOT reject the label for being outside a fixed set.

