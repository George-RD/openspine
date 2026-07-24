# Design: Wire production route/composition callsites to authority-equivalence selection

## Where the tie actually lives

`resolve_route` (AD-147's deterministic route conflict-resolution algorithm,
D-008: no LLM scoring) is the one place in the pipeline where multiple
authority candidates *compete* before composition: when two or more active
routes match an event with equal priority and equal `when` specificity, it
returns `RouteResolution::Ambiguous`. The pipeline driver (`run_pipeline_with_envelope`)
previously audited that and returned `Ok(None)` — dropping the event. That is
the production spot this change adopts (the brief: "Find the real production
spot where multiple authority candidates could compete").

## The equivalence path is already sealed (D-109/D-110)

`AuthorityEquivalenceClasses::compose_all` runs the *same* `compose_authority`
the kernel uses to mint a live grant and groups the resulting grants by their
five-field `AuthorityClassId`. `AuthorityEquivalenceClasses::resolve` returns
`Selected` for one known class and `Escalate` for more than one — a cross-class
pick is structurally impossible (the class-scoped selection type cannot return
a member of another class). This change *uses* that path; it does not alter it.

## Decision 1 — expose the tied candidates, not a synthetic fallback (D-124)

`RouteResolution::Ambiguous` gains `candidate_route_ids: Vec<ArtifactId>`,
populated by `resolve_route` from the already-computed `winners` (sorted +
deduped). This is the minimal surface the driver needs to drive equivalence
resolution and keeps `resolve_route`'s own algorithm unchanged (the existing
`fallback_route` field is retained for callers that still want it; no callers
rely on the documented-but-unwired fallback behavior).

## Decision 2 — resolve in a dedicated module and carry the selected snapshot (D-123/D-125/D-126/D-128/D-129)

`pipeline::route_ambiguity::resolve_tied_routes` does the adoption:

1. Assemble each candidate's authority sources from one registry read.
   Missing route/agent/workflow/pack/policy metadata escalates; silently
   dropping an invalid competitor could manufacture a false one-class result.
   A candidate whose pack does not `applies_to` the event remains a non-match,
   exactly like the single-route `pack_not_applicable` path.
2. Compose every remaining candidate through `compose_all` (sealed path). A
   composition failure among the competitors escalates — never "pick the rest."
3. `resolve` over the present classes. More than one class escalates. One class
   is eligible for deterministic within-class selection.
4. Before selection, compare the canonical composed `allowed_egress_classes`
   of every member. AD-147's frozen five-field class identity omits rated
   egress even though the gate enforces it; D-128 therefore refines D-110's
   production within-class consequence without changing `AuthorityClassId`.
5. Select index `0` (the lexicographically smallest candidate id because
   `from_candidates` sorts by id) and carry that member's exact composed grant
   plus route snapshot into the driver.

The driver never recomposes an ambiguous selection against the mutable live
registry. It resumes at the shared persona-binding and Grant-stage path with
the exact composition snapshot that passed resolution, closing an activation
TOCTOU window. The ordinary `RouteResolution::Success` fast path remains
unchanged and still composes once from the live registry.

## Decision 3 — escalation reuses the existing surface (D-110 reaffirmed; D-127)

A cross-class tie calls `failure_surfacing::notify_immediate_failure(state,
chat_id, FailureClass::Escalation, summary)` — the same immediate-owner surface
already used for the sibling `AuthorityOutcome::Ambiguous` case in the driver.
No new escalation transport is invented. The driver audits
`route.ambiguous.escalated` before notifying, and `route.ambiguous.not_applicable`
for the silent non-match path.

## Test mapping

| Requirement | Real test(s) |
| --- | --- |
| `resolve_route` exposes sorted/deduped tied candidate ids | `priority_tie_with_equal_specificity_is_ambiguous`, `three_way_priority_tie_exposes_sorted_deduped_candidate_ids` (authority crate) |
| Single class -> deterministic within-class pick through production path | `tied_authority_equivalent_routes_select_within_class` (kernel driver) |
| Cross-class tie escalates end-to-end via the owner surface | `tied_cross_class_routes_escalate_to_owner` (kernel driver) |
| Missing candidate metadata cannot be dropped to manufacture one class | `tied_route_with_missing_authority_metadata_escalates` (kernel driver) |
| Candidate composition failure cannot be dropped to manufacture one class | `tied_route_composition_failure_escalates` (kernel driver) |
| Differing effective egress cannot auto-select inside the frozen five-field class | `tied_routes_differing_only_in_egress_escalate` (kernel driver) |
| Registry activation after resolution cannot change the selected grant | `selected_class_persists_composition_snapshot_across_registry_update` (kernel driver) |
| All tied packs non-applicable -> audited silent non-match | `tied_routes_with_no_applicable_pack_are_silent_non_match` (kernel driver) |

## Adjudicated decisions

The seven new implementation candidates were accepted as D-123..D-129. The
cross-class escalation candidate reaffirms existing D-110.

- **D-123: The driver resolves an ambiguous route tie through
  `AuthorityEquivalenceClasses`, never by LLM scoring or a synthetic fallback.**
  `resolve_tied_routes` composes the tied candidates through the sealed
  `compose_all` and defers the final decision to `resolve`. Would change if:
  a future UX required the kernel to auto-pick across classes (forbidden — that
  would widen authority per D-110).
- **D-110 reaffirmed: A cross-class tie escalates to the owner through the existing
  immediate-notification surface; the driver never auto-selects across
  classes.** `FailureClass::Escalation` + `route.ambiguous.escalated` audit.
  Would change if: a new escalation transport were introduced (out of scope for
  adoption-only work).
- **D-127: A tie whose candidates all have a non-applicable pack is a
  silent non-match, not an escalation.** Mirrors the single-route
  `pack_not_applicable` drop. Would change if: the product wanted every tie
  surfaced (it does not — a non-match is not authority-relevant).
- **D-128: Rated egress equality is a production selection guard, not a
  sixth `AuthorityClassId` field.** AD-147 freezes the class identity to five
  fields, while the gate enforces composed egress. A mismatch therefore
  refines D-110's production selection consequence without changing D-109's
  class identity. Would change if: canon explicitly revises the class identity
  to include egress.
- **D-129: An ambiguous within-class selection persists the exact grant
  composed during class resolution.** It never recomposes against a newer
  registry snapshot, preventing artifact activation from changing authority
  between selection and persistence. Would change if: the registry gains a
  generation-pinned transaction spanning resolution and grant persistence.
