# Design: Authority-equivalence matcher (AD-147, AD-124)

## Kernel-owned class identity

`AuthorityClassId` is a deterministic projection of a kernel-composed
`TaskGrant`. It carries exactly the composed authority tuple named by
AD-147: `allowed_actions`, `approval_required_actions`,
`denied_actions`, `output_channels`, and `limits`. It deliberately
EXCLUDES grant identity, expiry, tokens, provenance, and egress
metadata — those are per-grant machinery, not authority shape, and
including them would make two authority-identical grants fall into
different classes.

Lists are sorted and deduplicated on construction because authority
composition emits canonical set-like lists; declaration order is not
authority (verified by `declared_list_order_does_not_change_class`).

## Sealed construction (no forged classes)

AD-147 requires the kernel — never the shell — to compute the
classes. The only public candidate builder is
`AuthorityEquivalenceClasses::compose_all`, which runs the SAME
`compose_authority` the kernel uses to mint a live grant and groups
the resulting `TaskGrant`s by their `AuthorityClassId`. The
`TaskGrant -> AuthorityCandidate` wrapper (`from_composed_grant`) is
`pub(crate)`, so a shell or LLM cannot hand-craft a `TaskGrant`
with an arbitrary class label. A candidate's class is therefore
always derived from real composition output.

## Class-scoped matcher view (cross-class pick impossible)

The semantic matcher receives only a `ResolvedAuthorityClass` view.
`ResolvedAuthorityClass::select_within_class` takes a closure that returns an
**index** into that class's own members; the returned
`AuthorityClassMember` is constructed solely from the class it was
selected from. There is no API that returns a member of a
different class, so a cross-class pick cannot be expressed by the
type. `AuthorityEquivalenceClasses::resolve` over multiple known
classes returns `Escalate` and never a member, so ambiguous
cross-class matches surface to the owner instead of widening
authority.

## Invariant preserved (refines D-008, never repeals it)

D-008: deterministic routing decides authority; agentic routing decides
strategy. This change does not let the matcher resolve route
conflicts or construct grants — `compose_authority` still owns both.
Within-class members are authority-identical BY CONSTRUCTION
(the class identity is the composed authority tuple), so the matcher's
free choice among them is taste, never authority. The property
test proves it: every within-class pick composes an identical grant,
and a cross-class pick is structurally impossible.

## Test mapping

| Requirement | Real test(s) |
| --- | --- |
| Kernel computes deterministic classes from declared action lists | `property_all_authority_dimensions_define_classes_and_identical_grants` |
| Class identity = identical composed (allowed/approval/denied/channels/limits) | `two_identical_inputs_form_one_class`, `declared_list_order_does_not_change_class` |
| Semantic matcher picks within one class, never across | `distinct_inputs_form_separate_classes_and_escalate` |
| Construction is sealed; unknown action id is a class error, never a widened class | `compose_denial_is_a_class_error` |

## Deferred / candidates (unnumbered)

- **(candidate) Authority-equivalence classes are computed by the
  kernel from composed grant projections, never from shell-supplied class
  identities.** `compose_all` is the sole public builder and
  routes through `compose_authority`; `from_composed_grant` is
  `pub(crate)`. Would change if: a matcher needed to group
  candidates before composition, or a class identity ever incorporated
  per-grant fields (id/token/expiry) — both violate AD-147's
  "identical composed tuple" definition.
- **(candidate) Cross-class ambiguity escalates to the owner; the
  matcher may never return a member of another class.** The type
  `AuthorityClassMember` is only constructable inside
  `select_within_class`, and `resolve` returns `Escalate` rather
  than a pick when more than one class matches. Would change if:
  a future UX required the kernel to auto-pick across classes
  (forbidden — that would widen authority).

Production adoption (route/composition callsites selecting via
`AuthorityEquivalenceClasses`) is explicitly deferred to
`wire-authority-equivalence-selection` (the first change whose scope owns
production route/composition adoption); tracked in
`openspec/openspine-change-sequence.md` at landing time.
