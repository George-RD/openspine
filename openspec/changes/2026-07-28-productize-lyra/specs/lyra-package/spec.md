# Lyra package

## Requirement: Lyra is a named default package

The repository SHALL contain a declarative package manifest identifying Lyra as
the default owner-facing agent package and naming its persistent entry agent.

### Scenario: Inspecting the source package

- **WHEN** an operator inspects `artifacts/lyra/package.yaml`
- **THEN** the manifest identifies `lyra`
- **AND** names `main_assistant_agent` as the entry agent
- **AND** states that the package is alpha.

## Requirement: Personality carries no authority

Lyra's human-readable identity contract and persona overlays SHALL NOT grant or
widen action authority.

### Scenario: Editing personality guidance

- **WHEN** personality guidance changes
- **THEN** effective permissions remain determined by policies, packs, approvals,
  caveats, runtime limits, and the task grant.

## Requirement: Memory is typed and scoped

The Lyra package SHALL state the memory classes and scopes its entry agent may
read and the sensitive classes it is denied.

### Scenario: Raw email body is considered for durable memory

- **WHEN** the main assistant requests durable access to `raw_email_body`
- **THEN** the agent manifest denies that class.

## Requirement: Installation remains declarative

A future native installer SHALL install a versioned package and atomically select
it rather than imperatively rewriting individual runtime artifacts.

### Scenario: Rolling back an installed package

- **WHEN** an operator selects a previous package version
- **THEN** OpenSpine loads that immutable package version
- **AND** does not attempt to reverse individual file mutations.
