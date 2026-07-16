# Design: define-lineage-and-eval-store

## Authority posture

This change is store groundwork and carries no live authority. Two
kernel invariants still bind the shape:

- **D-006 (identity-is-not-authority).** The eval-verdict row may carry an
  `evaluator` string as forward-compatible metadata. It is never a
  principal, never a grant, and never widens what a task may do. A verdict
  is an observation, not an authority object. Any evaluator-independence
  policy is deferred to the later evaluation change.
- **D-011 (digest-bound approval).** Each eval-verdict row carries
  `artifact_digest` — the digest of the evaluated bytes. The store does not
  re-compute digests; the column is the binding surface a later promotion
  decision will re-check.

Grant-is-the-only-live-authority (D-007), deny-by-default (D-004), and
shell containment (D-005) are unaffected: no new ActionId, no new
grant field, no shell-visible surface.

## Lineage model

```
ArtifactLineage {
  generation: u32,            // derivation depth; root = 0
  parents: Vec<LineageParent> // full identity of each parent
}
LineageParent { kind, artifact_id, version }
```

- **Distinct from `version`.** `version` (D-028) is content-edit history
  of one artifact. `generation` is derivation depth across artifacts.
  A derived artifact can be content-version 1 and generation 2.
- **Nullable on the row.** `proposed_artifacts.lineage_json` is
  nullable. `NULL` / `None` means provenance is *unknown* (legacy
  pre-lineage rows after the ad-hoc column migration). Unknown MUST
  NOT be rewritten as root — that would invent provenance. New inserts
  from `artifact.propose` supply `Some(ArtifactLineage::root())`
  explicitly.
- **Storage.** JSON-in-TEXT, matching the rest of the store's
  schema-as-JSON convention. No separate parent-edge table in this
  groundwork (children-of queries can be added later without breaking
  the column).

## Eval-verdict store

```
EvalVerdict {
  id, artifact_kind, artifact_id, artifact_version,
  verdict: String,            // open vocabulary
  fitness: Option<f64>,       // optional score
  evidence: Option<String>,   // forward-compatible supporting reference
  evaluator: Option<String>,  // metadata only; NOT authority (D-006)
  artifact_digest: String,   // D-011 binding
  recorded_at: i64,           // epoch nanoseconds; preserves ordering
}
```

- **Own table, not audit chain.** The non-retrofittable set forbids
  landing verdicts as audit-chain rows. `eval_verdicts` is a first-class
  indexed table.
- **Open verdict vocabulary.** The concrete verdict vocabulary is deferred.
  A closed enum would bake unsupported policy into this groundwork; a
  `String` keeps the surface open for the later evaluation change.
- **Optional fitness and evidence.** Not every evaluator emits a score;
  both fields remain optional. Their concrete semantics are deferred.
- **Indexes.**
  - `(artifact_kind, artifact_id, artifact_version, recorded_at)` —
    ordered history and latest-for-artifact.
  - `(verdict)` — filter by label across artifacts.
- **Append-only.** Re-evaluation inserts a new row;
  `latest_eval_verdict` returns the newest by `recorded_at`.
  `recorded_at` is persisted as checked epoch nanoseconds in an SQLite
  INTEGER so exact-second and fractional timestamps retain chronological
  ordering.

## Migration strategy

Ad-hoc, matching the existing `add_column_if_missing` pattern
(no `PRAGMA user_version` — owned by `implement-day2-operations`):

1. `proposed_artifacts::ensure_schema` creates the table with nullable
   `lineage_json` on fresh files.
2. `ALTER TABLE proposed_artifacts ADD COLUMN lineage_json TEXT` for
   existing files (no DEFAULT — legacy rows stay NULL).
3. `eval_verdict_store::ensure_schema` creates `eval_verdicts` + indexes.

## Alternatives considered

| Option | Why rejected |
|---|---|
| Closed `EvalVerdictKind` enum | Bakes unsupported policy; AD-111 vocabulary unsettled |
| Mandatory `f64` fitness | Some evaluators emit no score |
| Lineage as a separate table only | Done-when requires artifact *rows* carry lineage |
| `DEFAULT root()` on migration | Invents provenance for unknown legacy rows |
| Landing verdicts on the audit chain | Explicitly forbidden by the non-retrofittable set |

## Risks

- **Rebase hotspots.** `store/mod.rs` (module registration),
  `migrations.rs`, `schemas/lib.rs` (`pub mod lineage`). All edits are
  single-line or append-style.
- **File size.** `proposed_artifacts.rs` grows by the lineage column;
  stays well under 500 lines. New modules (`eval_verdict_store.rs`,
  `lineage.rs`) keep shared files small.
