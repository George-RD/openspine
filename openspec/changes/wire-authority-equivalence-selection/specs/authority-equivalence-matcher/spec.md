# authority-equivalence-matcher Specification (delta)

## ADDED Requirements

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
