# Proposal: Wire production route/composition callsites to authority-equivalence selection

## Why

`implement-authority-equivalence-matcher` (D-109/D-110; AD-147/AD-124) shipped
the kernel-owned `AuthorityEquivalenceClasses` machinery and explicitly
**deferred** production adoption to this change: "route/composition callsites
selecting via `AuthorityEquivalenceClasses`" was left as "explicitly deferred
to `wire-authority-equivalence-selection`." Today `resolve_route` reports a
multi-match tie as `RouteResolution::Ambiguous`, but the pipeline driver simply
audits it and drops the event — it never uses the tie to make a safe,
authority-bounded decision.

This change closes that gap. When `resolve_route` cannot order competing
candidate routes, the driver resolves the tie through the sealed equivalence
path instead of giving up: candidates that compose into a single authority
class with identical canonical egress are picked deterministically within that
class; candidates that span classes or effective egress sets escalate to the
owner and are never auto-picked.

## What Changes

- Extend `RouteResolution::Ambiguous` (in `openspine-schemas`) to also carry
  `candidate_route_ids` — the tied winners, sorted and deduped by `resolve_route`
  (no new conflict-resolution logic; D-008's deterministic algorithm is untouched).
- Add `openspine-kernel::pipeline::route_ambiguity`: `resolve_tied_routes`
  composes each applicable tied candidate through the same kernel
  `compose_authority`, groups them by `AuthorityClassId`, and asks
  `AuthorityEquivalenceClasses::resolve` to decide (single class -> deterministic
  within-class pick; multiple classes -> escalate). Invalid candidate metadata
  or composition escalates instead of shrinking the competing set.
- Fail closed when one nominal class contains different composed egress sets.
  Rated egress is effective gate authority but not one of AD-147's frozen five
  identity fields, so this guard does not redefine `AuthorityClassId`.
- Wire `run_pipeline_with_envelope`'s `RouteResolution::Ambiguous` branch to
  persist the exact composed grant snapshot selected by `resolve_tied_routes`.
  It never recomposes against a potentially newer live registry. Escalation
  reuses `failure_surfacing::notify_immediate_failure`; an all-non-applicable
  tie remains a silent non-match.

This is **adoption only**: no new authority semantics, no new matcher behavior.
The adoption decisions discovered during implementation are ratified as
D-123..D-129; cross-class escalation reaffirms D-110.
The single-candidate fast path (`RouteResolution::Success`) is untouched.

## Acceptance Criteria

- A production routing path (the pipeline driver) selects a tied route through
  class resolution under test: `tied_authority_equivalent_routes_select_within_class`
  persists exactly one grant and does not escalate.
- Cross-class ambiguity escalates end-to-end under test:
  `tied_cross_class_routes_escalate_to_owner` audits `route.ambiguous.escalated`,
  fires the owner-notification surface, and persists no grant.
- Invalid tied authority metadata cannot be dropped to manufacture an
  apparently safe single class; it escalates and persists no grant.
- Tied candidates differing only in composed egress escalate under
  `tied_routes_differing_only_in_egress_escalate`.
- `selected_class_persists_composition_snapshot_across_registry_update` proves
  a live registry replacement cannot change the selected grant before persist.
- `tied_routes_with_no_applicable_pack_are_silent_non_match` covers the
  audited zero-grant, zero-notification non-match branch.
- Rust formatting, lint (`-D warnings`), tests, file-size, claims, and strict
  OpenSpec validation are green.
- No `.raw`/sequence-ledger/scripts edits; no production `unwrap`/`expect`.
