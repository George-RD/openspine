# openspine-development-process Specification Delta

## ADDED Requirements

### Requirement: The checked capability map MUST separate generic capability, selected proof, and portability evidence

The checked capability map MUST represent a generic capability, a selected
vertical proof, portability evidence, and whole-responsibility maturity as
distinct schema objects, so that documentation cannot make the vertical proof
stand in for the generic capability or for cross-protocol portability. A
generic capability MUST be a protocol-neutral capability record. A selected
proof MUST be a separate proof record that references a capability id. Portability
evidence MUST be a separate proof record distinct from the first selected
proof. Whole-responsibility composition MUST be represented as a later maturity
stage, never inferred from a single effect shortcut. A capability map that
names the recurring Gmail draft as both the missing capability and the selected
proof MUST be rejected, because it conflates the generic capability with one
vertical proof.

#### Scenario: The map names the capability and the proof as the same object

- **WHEN** a capability map record is both the missing generic capability and
  the selected proof
- **THEN** the capability map MUST be rejected
- **AND** the map MUST instead carry a generic capability and a separate
  selected proof record that references it.

#### Scenario: A proof advances without rewriting the generic identity

- **WHEN** the selected proof, portability proof, or whole-responsibility
  progression updates its own evidence fields
- **THEN** the generic capability identity MUST remain unchanged.

#### Scenario: The map represents whole-responsibility maturity

- **WHEN** the capability map records a whole-responsibility progression
- **THEN** it MUST be represented as a distinct maturity stage
- **AND** it MUST NOT be marked verified from a single effect shortcut alone.

### Requirement: Issue numbers MUST be blockers, never runtime evidence

A capability map MUST treat issue numbers as references that explain blockers
and open work. An issue number MUST NOT be used as runtime evidence that a
behavior has landed. Landed behavior MUST be proved only by archived changes,
spec paths, artifact paths, and named tests that resolve to the repository. The
capability map MUST distinguish blocker references from landed-substrate
evidence.

#### Scenario: An issue number is used where evidence is expected

- **WHEN** an issue number appears in a field that the capability map treats as
  runtime evidence, such as a runtime-change list or an owner-path test list
- **THEN** the capability map MUST be rejected.

#### Scenario: A blocker is recorded without claiming it shipped

- **WHEN** a capability records an open issue as a blocker
- **THEN** the issue MUST render as a blocker, never as a shipped feature.

### Requirement: A wired generic capability MUST have positive-effect and fallback/control owner-path tests

A generic capability MUST NOT be marked wired into Lyra unless it names at
least one owner-path test for a real positive effect and at least one
owner-path test for a required fallback or control case. A proposal renderer,
a standing-rule activation test, or a runtime primitive alone MUST NOT satisfy
this requirement. Every named owner-path test MUST resolve to a registered Rust
test in the repository.

#### Scenario: A generic capability is wired on runtime changes alone

- **WHEN** a generic capability is marked wired into Lyra but has no
  positive-effect owner-path test or no fallback/control owner-path test
- **THEN** the capability map MUST be rejected.

#### Scenario: A generic capability is wired with categorized owner-path tests

- **WHEN** a generic capability is marked wired into Lyra
- **THEN** it MUST name at least one positive-effect owner-path test and at
  least one fallback or control owner-path test
- **AND** each named test MUST resolve to a registered Rust test.

### Requirement: Portability MUST NOT be marked verified without second-protocol evidence

Portability evidence MUST NOT be marked verified until a second materially
different protocol has demonstrated conformance through a named conformance
test suite. A portability proof marked verified MUST carry non-empty conformance
evidence.

#### Scenario: Portability is verified without conformance evidence

- **WHEN** a portability proof is marked verified but carries no conformance
  tests
- **THEN** the capability map MUST be rejected.

#### Scenario: The first shipped proof does not imply cross-protocol portability

- **WHEN** the selected proof is shipped but the portability proof is not
  verified
- **THEN** the public copy MUST describe the shipped proof as the first shipped
  proof
- **AND** it MUST NOT claim the capability works across protocols.

### Requirement: Whole-responsibility maturity MUST depend on shipped proof and verified portability

A whole-responsibility progression MUST NOT be marked verified unless the
selected proof is shipped and the portability proof is verified. The map MUST
record the dependency between the whole-responsibility maturity stage, the
selected proof, and the portability proof.

#### Scenario: Whole-responsibility is verified before its dependencies

- **WHEN** a whole-responsibility progression is marked verified while the
  selected proof is not shipped or the portability proof is not verified
- **THEN** the capability map MUST be rejected.

### Requirement: The capability map MUST record a landed-substrate and blocker structure

A capability record MUST list its landed substrate through archived runtime
changes and canonical spec paths, and MUST list its open blockers separately as
issue references. The selected proof, portability proof, and whole-responsibility
progression MUST each be linked to their tracking issues.

#### Scenario: A capability has landed substrate and open blockers

- **WHEN** a capability lists landed substrate and open blockers
- **THEN** the landed substrate MUST be expressed as archived changes and spec
  paths
- **AND** the open blockers MUST be expressed as issue references.

#### Scenario: The roadmap renders the dependency structure

- **WHEN** the public roadmap is generated from the capability map
- **THEN** it MUST render the generic capability with its landed substrate,
  its role-grouped blockers, and its selected, portability, and
  whole-responsibility maturity lines
- **AND** it MUST render blockers as issue links, not as repository proof.

### Requirement: The generated roadmap MUST be deterministic and replace only its generated block

The public roadmap MUST be generated deterministically from the capability map.
The generated block between the capability-map start and end markers in
`site/src/content/docs/roadmap.md` MUST be replaced only within those markers;
content outside the markers MUST remain unchanged. The read-only validation
MUST fail if the generated output is stale.

#### Scenario: The roadmap is stale

- **WHEN** the checked roadmap content between the markers differs from the
  generated output for the current capability map
- **THEN** validation MUST fail
- **AND** regeneration MUST replace only the content inside the markers.

### Requirement: The capability-map schema MUST migrate explicitly and idempotently

Changes to the capability-map schema MUST ship an explicit, tested, idempotent
migration from the prior schema version. Re-running the migration on an
already-current map MUST be a no-op.

#### Scenario: A version 1 map is migrated to version 2

- **WHEN** a version 1 capability map is migrated
- **THEN** the result MUST be a structurally valid version 2 map with a top-level
  proofs array, `generic` and `blocking_issues` fields present on every
  capability, and the selected proof preserved as its own separate record
- **AND** migrating the already-current map MUST be a no-op.
- **AND** authoring the generic, protocol-neutral capability is a separate
  deliberate act, not a mechanical transform: the migration MUST NOT invent a
  generic capability out of a vertical one.
