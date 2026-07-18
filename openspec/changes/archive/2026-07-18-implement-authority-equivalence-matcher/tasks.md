# Tasks

- [x] Add `openspine_authority::equivalence`: `AuthorityClassId` (deterministic projection of a composed grant's authority tuple), `AuthorityCandidate` (kernel-composed candidate), `AuthorityEquivalenceClasses` (auditable grouping).
- [x] Seal construction: `AuthorityEquivalenceClasses::compose_all` runs `compose_authority`; `AuthorityCandidate::from_composed_grant` is `pub(crate)`, so no shell/LLM can label an arbitrary `TaskGrant` with a forged class.
 - [x] Expose `ResolvedAuthorityClass::select_within_class` (returns only a member of that class) and `AuthorityEquivalenceClasses::resolve` (escalates on multiple known classes, never returns a member).
 - [x] Write the property test (`property_all_authority_dimensions_define_classes_and_identical_grants`): every within-class pick composes a grant whose authority fields equal the class baseline; cross-class resolution escalates and never returns a member.
- [x] Write named invariant tests: `two_identical_inputs_form_one_class`, `distinct_inputs_form_separate_classes_and_escalate`, `declared_list_order_does_not_change_class`, `compose_denial_is_a_class_error`.
- [x] Write OpenSpec change artifacts (proposal/design/tasks + `authority-equivalence-matcher` delta spec with `## ADDED Requirements` and `#### Scenario:` blocks, each scenario mapping 1:1 to a real test).
- [x] Run all required local gates and record evidence.
- Gate evidence: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `scripts/check-file-sizes.sh`, `scripts/check-claims.sh`, `scripts/check-omp-ceremony.sh`, and `openspec validate implement-authority-equivalence-matcher --strict` all pass (see `IMPLEMENTATION-NOTES.md`).

Production adoption (route/composition callsites selecting via
`AuthorityEquivalenceClasses`) is explicitly deferred to
`wire-authority-equivalence-selection` (the first change whose scope owns
production route/composition adoption); tracked in
`openspec/openspine-change-sequence.md` at landing time.
