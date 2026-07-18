# artifact-lifecycle Specification

## ADDED Requirements

### Requirement: Personality seed artifacts MUST load as overlay learned artifacts with provenance

The Donna×Leo personality seed (AD-080) MUST ship as pre-populated, learnable
`persona` overlay artifacts — never as base/kernel-baked fixtures. Each seed
artifact MUST load into the artifact registry and MUST carry a non-null
`ProducedBy` exchange provenance (D-077) so it enters the effective registry
through the same overlay machinery as any owner-learned artifact.

#### Scenario: Seed elements load into the registry with ProducedBy provenance

- **GIVEN** a kernel boot with an empty `learned_artifacts` store
- **WHEN** the personality seed bootstrap runs and the overlay is loaded
- **THEN** the registry contains the eight AD-080 elements plus the AD-082
  digest/brief default as `persona` artifacts
- **AND** each loaded persona row has `namespace = overlay` and
  `Provenance::ProducedBy` with a non-null source event and encrypted exchange digest

#### Scenario: Seed survives a kernel restart

- **GIVEN** the personality seed has been written to `data/artifacts.d/personas`
  and recorded in `learned_artifacts`
- **WHEN** the kernel boots again
- **THEN** the persona artifacts are present in the registry on the second boot
- **AND** no duplicate `learned_artifacts` rows are created for the same
  `(kind, artifact_id, version)`

### Requirement: Personality seed MUST NOT be kernel-baked base fixtures

Persona seed artifacts MUST live only in the overlay namespace. The loader MUST
NOT source them from the base fixture directory, and the kernel MUST NOT treat
a seeded persona as a base artifact during the base/overlay compatibility pass.

#### Scenario: Seed is excluded from base identity and compatibility

- **GIVEN** a seeded persona artifact in the overlay
- **WHEN** the compatibility pass and the base-identity collision check run
- **THEN** the persona is present only in the overlay namespace
- **AND** it is never counted as a base identity or flagged as a base collision

### Requirement: Personality seed seeding MUST be idempotent across boots

The seed bootstrap MUST treat a persona as converged only when its
`learned_artifacts` row exists and its on-disk YAML is present with the row's
recorded digest. It MUST durably write and fsync each element's YAML before
recording its provenance row, and MUST converge repeated or partially-completed
boots without duplicate rows or dangling provenance.

#### Scenario: A second boot seeds nothing new

- **GIVEN** the seed has already written all nine persona elements
- **WHEN** the seed bootstrap runs again on a fresh boot
- **THEN** no new `learned_artifacts` rows are inserted
- **AND** the overlay directory still contains exactly the nine seeded files

#### Scenario: A crash between file write and provenance row self-heals

- **GIVEN** the seed wrote a persona YAML file but the kernel crashed before
  recording its provenance row
- **WHEN** the kernel boots again and the seed bootstrap runs
- **THEN** the missing element is written and recorded, converging to the full set

### Requirement: Every AD-081/AD-083 anti-pattern MUST have an eval probe

The eval harness MUST define a deterministic probe for each personality
anti-pattern: the seven from AD-081 (deferential double-asking, sycophancy,
over-explaining, nagging, presumptuous anticipation, need-to-know failure,
apology theater) and the three AD-083 additions (faked intimacy, info-dump
without synthesis, self-promotional visibility).

#### Scenario: All ten anti-patterns are represented

- **GIVEN** the eval-harness probe registry
- **WHEN** it is enumerated
- **THEN** it contains exactly one probe for each of the ten AD-081/AD-083
  anti-patterns

### Requirement: Anti-pattern probes MUST fail on violating output and pass on clean output

A personality anti-pattern probe MUST return a violation when the
output-under-test exhibits that anti-pattern, and MUST return no violation on
clean output, so the eval harness can fail any violating sample.

#### Scenario: A violating sample trips its probe

- **GIVEN** an output string that exhibits one AD-081/AD-083 anti-pattern
- **WHEN** the corresponding probe runs against it
- **THEN** the probe returns a violation naming that anti-pattern

#### Scenario: Clean output trips no probe

- **GIVEN** an output string with none of the AD-081/AD-083 anti-patterns
- **WHEN** all probes run against it
- **THEN** no probe returns a violation

#### Scenario: A committed row with a missing or corrupt file self-heals

- **GIVEN** a canonical persona `learned_artifacts` row exists but its YAML is
  missing or its bytes no longer match the canonical seed digest
- **WHEN** the kernel boots and the personality seed bootstrap runs
- **THEN** the seed republishes the canonical seed content when the row has
  valid, resolvable `ProducedBy` provenance
- **AND** it leaves that valid provenance row untouched
- **AND** a row with a dangling event, unbound exchange, or foreign digest is
  quarantined and reseeded rather than blocking the canonical persona forever

#### Scenario: Learned row and seeded receipt are atomic

- **GIVEN** the durable YAML has been published and the `personality_seed.seeded`
  audit append fails
- **WHEN** the seed transaction rolls back
- **THEN** neither the persona `learned_artifacts` row nor its `personality_seed.seeded`
  receipt remains committed
- **AND** a later boot can retry the element without a permanently missing receipt

### Requirement: Persona artifacts MUST never enter kernel authority

`persona` MUST remain absent from the proposable-kind table and MUST NOT have a
`ParsedProposal` representation. The propose and upstream-nomination paths MUST
reject `persona`, and no persona artifact may enter grant, gate, approval, or
activation authority.

#### Scenario: Persona proposal and nomination are rejected

- **GIVEN** a caller submits a `persona` proposal or upstream nomination
- **WHEN** the kernel validates the artifact kind
- **THEN** it returns a structured bad request
- **AND** it records no proposed artifact, approval request, grant, or activation

### Requirement: Seed guidance MUST keep negative constraints in eval probes

Seeded `PersonaElement.guidance` MUST be positive desired behavior only. AD-054
anti-pattern names, citations, and negative/meta-guidance MUST remain exclusively
in deterministic eval probes, never in shipped persona guidance.

#### Scenario: Seed guidance contains no probe-only constraint text

- **GIVEN** the nine seeded persona elements
- **WHEN** their guidance is loaded
- **THEN** no guidance contains AD-081/AD-083 anti-pattern names or citations
- **AND** `run_probes` remains the sole executable negative-constraint boundary

### Requirement: Digest/brief format MUST remain a learnable default

The AD-082 digest/brief shape MUST ship as an overridable persona overlay
default, not as an enforced schema field, immutable prompt authority, or
kernel grant/gate rule. This change MUST NOT claim an owner-correction or
miner/proposal route that it does not implement; `implement-reflection-miner`
owns that later correction→miner→proposal route while this change preserves
the default-only boundary.

#### Scenario: Digest default is presentation guidance only

- **GIVEN** the `digest_brief_default` persona element
- **WHEN** the registry loads it
- **THEN** it is a normal learnable `persona` overlay artifact whose guidance
  can be replaced by a future correction lane
- **AND** no schema validation, proposal authority, grant, or gate enforces its
  presentation text as an immutable format
