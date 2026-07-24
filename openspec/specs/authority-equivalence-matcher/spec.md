# authority-equivalence-matcher Specification

## Purpose
TBD - created by archiving change implement-authority-equivalence-matcher. Update Purpose after archive.
## Requirements
### Requirement: Kernel computes authority-equivalence classes deterministically from declared action lists

The kernel — never the shell — MUST compute equivalence classes from
composed grants. Two candidates are in one class iff their composed
`(allowed_actions, approval_required_actions, denied_actions,
output_channels, limits)` are identical (AD-147). The class
identity MUST be derived from `compose_authority` output, never from
a shell- or LLM-supplied class label.

#### Scenario: Within-class picks compose an identical grant

- **WHEN** candidates share a composed authority tuple and a
  semantic matcher selects any member of the class
- **THEN** every selected member's grant MUST have identical
  `allowed_actions`, `approval_required_actions`, `denied_actions`,
  `output_channels`, and `limits` to the class baseline, so the
  pick cannot widen authority
  (`test: property_all_authority_dimensions_define_classes_and_identical_grants`)

### Requirement: Class identity equals the composed authority tuple

Class identity MUST be exactly the composed
`(allowed_actions, approval_required_actions, denied_actions,
output_channels, limits)`. Declaration ORDER MUST NOT change
class identity; only the set of composed actions and channels
and the limits matter (AD-147).

#### Scenario: Identical declared sets form one class

- **WHEN** two candidates declare the same action lists (in any
  order) and compose
- **THEN** they MUST fall into exactly one authority class
  (`test: two_identical_inputs_form_one_class`)

#### Scenario: Declaration order does not change class identity

- **WHEN** two candidates declare the same action set in different
  orders
- **THEN** they MUST share one class identity, proving the
  matcher compares composed sets, not declaration order
  (`test: declared_list_order_does_not_change_class`)

### Requirement: The matcher picks only within one class; a cross-class pick is structurally impossible

The semantic matcher MUST receive only a class-scoped view and
MUST return a member of exactly that class. It MUST NOT be
able to return a member of a different class; cross-class
ambiguity MUST resolve by deterministic rule or escalation (AD-147).
The class-scoped selection return type MUST be constructible
only from the chosen class, so a cross-class pick cannot be
expressed by the type.

#### Scenario: Distinct classes escalate and never cross-pick

- **WHEN** more than one known class matches a semantic query
- **THEN** resolution MUST escalate and MUST NOT return a member
  of any class, so a cross-class pick is structurally impossible
  (`test: distinct_inputs_form_separate_classes_and_escalate`)

### Requirement: Class construction is sealed to the kernel composition path

The only public candidate builder MUST run `compose_authority`
(the same kernel function that mints a live grant). A shell or
LLM MUST NOT be able to label an arbitrary `TaskGrant` with a
forged class. An unknown action id MUST surface as a class
construction error, never as a silently-widened class (D-053).

#### Scenario: Unknown action id is a class error, never a widened class

- **WHEN** a candidate's declared action id is not in the canonical
  `ActionCatalog`
- **THEN** class construction MUST fail with a structured error
  and MUST NOT mint a class that smuggles the unknown id into
  authority (`test: compose_denial_is_a_class_error`)

### Requirement: Production route/composition callsites resolve ambiguous ties through authority-equivalence classes

When `resolve_route` reports an ambiguous multi-match tie
(`RouteResolution::Ambiguous`), the production pipeline driver MUST resolve the
tie through the kernel-owned `AuthorityEquivalenceClasses` path rather than
dropping the event. It MUST compose each applicable tied candidate through the
same `compose_authority` the kernel uses to mint a live grant, group them by
their `AuthorityClassId`, and then select deterministically when exactly one
class results or escalate to the owner when more than one class results.

Because rated egress is effective gate authority but is not one of AD-147's
frozen five class fields, the driver MUST additionally require identical
canonical `allowed_egress_classes` across every member of a selected class and
MUST escalate on any mismatch. A successful selection MUST carry the exact
composed grant snapshot into persistence and MUST NOT recompose it from a
potentially newer live registry. This is adoption only: it MUST NOT introduce
a new class identity or permit a cross-authority pick (D-109/D-110,
D-123..D-129).
D-128 refines D-110's production within-class consequence by requiring this
egress homogeneity guard while leaving D-109's class identity unchanged.

#### Scenario: A tied authority-equivalent set selects within the class in production

- **WHEN** two or more routes tie on priority and specificity, every tied
  candidate composes into the same `AuthorityClassId`, and their canonical
  composed `allowed_egress_classes` are identical
- **THEN** the driver MUST select one candidate deterministically (lowest
  candidate id) and compose and persist exactly one task grant, and MUST NOT
  escalate (`test: tied_authority_equivalent_routes_select_within_class`)

#### Scenario: A cross-class tie escalates end-to-end

- **WHEN** two or more routes tie on priority and specificity, and the tied
  candidates compose into more than one `AuthorityClassId`
- **THEN** the driver MUST NOT auto-select any candidate, MUST audit the tie
  as an escalation, and MUST reach the owner through the existing immediate
  notification surface, persisting no grant
  (`test: tied_cross_class_routes_escalate_to_owner`)

#### Scenario: Missing tied candidate metadata fails closed

- **WHEN** a tied candidate references missing authority metadata
- **THEN** the driver MUST NOT drop that competitor and select from the
  remainder; it MUST escalate to the owner and persist no grant
  (`test: tied_route_with_missing_authority_metadata_escalates`)

#### Scenario: Tied candidate composition failure fails closed

- **WHEN** one tied candidate fails composition
- **THEN** the driver MUST NOT drop that competitor and select from the
  remainder; it MUST escalate to the owner and persist no grant
  (`test: tied_route_composition_failure_escalates`)

#### Scenario: Equivalent five-field classes with different egress escalate

- **WHEN** tied candidates share one `AuthorityClassId` but their composed
  `allowed_egress_classes` differ
- **THEN** the driver MUST escalate to the owner and persist no grant
  (`test: tied_routes_differing_only_in_egress_escalate`)

#### Scenario: Selected composition snapshot survives a live registry update

- **WHEN** the selected route's authority source changes after equivalence
  resolution but before grant persistence
- **THEN** the returned and persisted grant MUST retain the exact authority
  snapshot that passed class resolution
  (`test: selected_class_persists_composition_snapshot_across_registry_update`)

#### Scenario: All non-applicable tied candidates are a silent non-match

- **WHEN** every tied candidate's capability pack is non-applicable to the
  event
- **THEN** the driver MUST audit the non-match, persist no grant, and MUST NOT
  notify the owner
  (`test: tied_routes_with_no_applicable_pack_are_silent_non_match`)

