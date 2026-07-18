# persona-binding Specification

## Purpose
TBD - created by archiving change implement-persona-binding-and-headless-lanes. Update Purpose after archive.
## Requirements
### Requirement: Kernel resolves persona fronting at route time
The kernel MUST resolve an active persona only from the winning deterministic route and resolved identity context; agents and overlays MUST NOT choose the fronting persona.

#### Scenario: Owner bound number receives owner persona
- **GIVEN** an active route for the owner's bound channel account and an active persona reference
- **WHEN** the owner event reaches ROUTE
- **THEN** the resulting grant carries the route persona reference

#### Scenario: Counterparty cannot inherit owner persona
- **GIVEN** a counterparty event that does not match the owner-bound route
- **WHEN** ROUTE resolves candidates
- **THEN** no owner persona is selected

