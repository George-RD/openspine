# Change: define-lineage-and-eval-store

## Dependencies

None (`Requires:` empty in the change sequence).

## Problem/Context

The agent-OS design log's non-retrofittable set requires two pieces of
schema/store groundwork before any generation or evaluation feature can
ship without backfill:

1. A **generation/lineage model** for artifacts, distinct from the existing
   content `version: u32` (D-028). Version tracks edits to one artifact;
   lineage tracks how an artifact came to be (parent refs + derivation
   depth).
2. An **eval-verdict/fitness store** as its own indexed tables — not as
   audit-chain rows. AD-111 (*leaning* — cited only for verdict landing)
   states that verdicts land in the eval store; that landing surface must
   exist first.

Without these, any later generation or promotion-review change would have
to retroactively invent columns and invent provenance for already-written
rows — exactly what the non-retrofittable set forbids.

## Proposed Solution

- Add `ArtifactLineage` / `LineageParent` pure-data types to
  `openspine-schemas` (deny_unknown_fields; generation distinct from
  version).
- Add a nullable `lineage_json` column on `proposed_artifacts` so artifact
  rows can carry lineage. `NULL` means provenance is *unknown* (legacy
  pre-lineage rows) and is never silently rewritten as root. New inserts
  supply an explicit `Some(ArtifactLineage)` (fresh proposals use
  `Some(root())`).
- Add an `eval_verdicts` indexed table with store APIs:
  `insert_eval_verdict`, `eval_verdicts_for_artifact`,
  `eval_verdicts_by_verdict`, `latest_eval_verdict`. Verdict vocabulary is
  an open string; fitness and evidence are optional forward-compatible
  fields. Evaluator identity is metadata only and is never authority
  (D-006).

No pipeline behaviour change: this is schema + store groundwork only.

## Acceptance Criteria

- Schema types for lineage exist with tests (root, derived, serde,
  deny_unknown_fields, generation ≠ version).
- Artifact rows round-trip lineage (root, derived, and unknown/`None`).
- Eval verdicts insert and query via indexed tables (by artifact ordered,
  by verdict label, latest-for-artifact).
- `openspec validate define-lineage-and-eval-store --strict` passes.
- Local gate green: `cargo fmt --check`, `clippy -D warnings`,
  `cargo test --workspace`, `scripts/check-file-sizes.sh`.

## Out of Scope

- Pipeline / generation behaviour that *produces* derived artifacts.
- AD-111 prover-judge implementation and concrete verdict vocabulary.
- Promotion / autonomy-ladder policy that *consumes* eval verdicts.
- Versioned `PRAGMA user_version` migrations (owned by
  `implement-day2-operations`).
- Audit-chain changes — the eval store is explicitly not the audit chain.
