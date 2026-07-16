# Tasks: define-lineage-and-eval-store

- [x] Add `ArtifactLineage` / `LineageParent` to `openspine-schemas` with tests
- [x] Add nullable `lineage_json` column + migration on `proposed_artifacts`
- [x] Round-trip lineage through insert/find (root, derived, unknown)
- [x] Pass `Some(ArtifactLineage::root())` from `artifact.propose`
- [x] Add `eval_verdicts` table + indexes + store APIs
- [x] Tests: insert/query by artifact, by verdict, latest-for-artifact, open vocabulary
- [x] OpenSpec proposal / design / specs delta
- [x] Local gate (fmt / clippy / test / file-sizes) + `openspec validate --strict`
