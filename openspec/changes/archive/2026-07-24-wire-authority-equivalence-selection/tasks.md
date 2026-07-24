# Tasks

- [x] Extend `RouteResolution::Ambiguous` with `candidate_route_ids` (sorted, deduped tied winners), populated by `resolve_route`; keep `fallback_route` for backward callers.
- [x] Add `pipeline::route_ambiguity` (`resolve_tied_routes` + `TieResolution`): assemble applicable tied candidates from one registry snapshot, fail closed on missing metadata/composition or differing effective egress, compose through sealed `AuthorityEquivalenceClasses::compose_all`, and decide via `resolve`.
- [x] Wire `run_pipeline_with_envelope`'s `RouteResolution::Ambiguous` branch to persist the exact selected composition snapshot without recomposing against the mutable registry; reuse the existing audited owner escalation surface and preserve all-non-applicable as a silent non-match.
- [x] Add authority-crate unit tests: `priority_tie_with_equal_specificity_is_ambiguous` asserts sorted/deduped `candidate_route_ids`; add `three_way_priority_tie_exposes_sorted_deduped_candidate_ids`.
- [x] Add kernel driver end-to-end tests for deterministic within-class selection, cross-class owner escalation, missing metadata, composition failure, differing egress, registry-update snapshot stability, and all-non-applicable silent non-match; each delta scenario maps 1:1 to one real test.
- [x] Write OpenSpec change artifacts (proposal/design/tasks + `authority-equivalence-matcher` delta spec with `## ADDED Requirements` and `#### Scenario:` blocks, each scenario mapping 1:1 to a real test).
- [x] Run all required local gates and record evidence.
- Gate evidence: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `scripts/check-file-sizes.sh`, `scripts/check-claims.sh`, `scripts/check-omp-ceremony.sh`, and `openspec validate wire-authority-equivalence-selection --strict` all pass (see `IMPLEMENTATION-NOTES.md`).
