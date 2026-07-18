# Proposal: Implement the authority-equivalence matcher

## Why

AD-147 (settled) confines the semantic matcher to authority-equivalence
classes so it can choose tastefully without ever widening authority.
AD-124 (settled) established that planners/matchers live in the shell
and PROPOSE, while the kernel materializes grants deterministically.
Today `compose_authority` mints a `TaskGrant` but nothing groups
authority-identical candidates, so a matcher has no kernel-bounded
vocabulary to pick from: it would either re-derive authority (forbidden
by D-008) or accept a shell-supplied class label (forgeable).

This change adds the kernel-owned equivalence-class machinery the matcher
consumes: a deterministic class identity computed from a composed
grant's authority tuple, a sealed construction path that runs the same
`compose_authority` the kernel uses, and a class-scoped selection
view that makes a cross-class pick structurally impossible.

## What Changes

- Add `openspine_authority::equivalence`: `AuthorityClassId` (the
  deterministic projection of a composed grant's authority tuple),
  `AuthorityCandidate` (a kernel-composed candidate), and
  `AuthorityEquivalenceClasses` (the auditable grouping).
- Seal construction: the only public candidate builder is
  `AuthorityEquivalenceClasses::compose_all`, which runs
  `compose_authority` over each input and groups the resulting
  grants by class identity. `AuthorityCandidate::from_composed_grant`
  is `pub(crate)`, so no shell or LLM can label an arbitrary
  `TaskGrant` with a forged class.
 - Expose `ResolvedAuthorityClass::select_within_class`, which accepts only an
 index into that class's own members; the returned
 `AuthorityClassMember` is derived solely from the chosen class.
  `AuthorityEquivalenceClasses::resolve` returns `Escalate` over
  multiple known classes and never a member, so a cross-class
  pick cannot be expressed by the type.


Production adoption (route/composition callsites selecting via
`AuthorityEquivalenceClasses`) is explicitly deferred to
`wire-authority-equivalence-selection` (the first change whose scope owns
production route/composition adoption); tracked in
`openspec/openspine-change-sequence.md` at landing time.
## Acceptance Criteria

- Property test: any within-class pick composes an IDENTICAL grant
  (equal `allowed_actions` / `approval_required_actions` /
  `denied_actions` / `output_channels` / `limits`); a cross-class
  pick is structurally impossible.
- Rust formatting, lint (`-D warnings`), tests, file-size, claims,
  and strict OpenSpec validation are green.
- No `.raw`/sequence-ledger/scripts edits; no production `unwrap`.
