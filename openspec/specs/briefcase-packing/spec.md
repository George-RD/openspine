# briefcase-packing Specification

## Purpose
TBD - created by archiving change implement-briefcase-packing. Update Purpose after archive.
## Requirements
### Requirement: Kernel packs every task deterministically
The kernel MUST derive a briefcase from route, workflow, and truthful counterparty shape, including the live grant projection, relevant preferences, relevant skills, and counterparty slice. The grant projection MUST include the grant's allowed egress classes.

#### Scenario: Identical task shape and snapshot
- **WHEN** the kernel packs the same shape from the same source snapshot twice
- **THEN** canonical serialized bytes are identical

#### Scenario: Independent grants with identical semantics
- **WHEN** two grants with different instance-only fields (id, token, timestamps) but identical semantic fields are packed
- **THEN** canonical serialized bytes are identical

### Requirement: Depth limits packed content
The kernel MUST compute briefcase depth as a deterministic function of relationship tier and task class, and MUST use that depth to limit the number of relevant preferences and skills packed into the worker's context.

#### Scenario: Stranger conversation pack is leaner
- **WHEN** a stranger/conversation task and an owner/effectful task draw from the same source pool
- **THEN** the stranger pack contains strictly fewer preference and skill sections

### Requirement: Every task is packed before worker spawn
The kernel MUST pack and persist a briefcase for every composed grant after grant binding and before the Run stage spawns a worker. A packing or persistence failure MUST abort the task.

#### Scenario: Packing failure aborts
- **WHEN** briefcase packing or persistence fails
- **THEN** no worker is spawned

### Requirement: Visibility classes are enforced structurally
The kernel MUST classify sections as kernel-bound, worker-scratch, or returned-output and persist a per-worker visibility record.

#### Scenario: Worker receives a view
- **WHEN** a worker view is projected
- **THEN** kernel-bound sections are absent even if requested

#### Scenario: Worker result is exported
- **WHEN** returned output requests a worker-scratch or kernel-bound key
- **THEN** export is denied

### Requirement: Top-ups are kernel-mediated and gate-visible
A worker or master MAY submit a top-up request, but the kernel MUST resolve the relevant source, bind its digest, apply policy, and mutate the briefcase. Every top-up decision (allowed or denied) MUST be recorded in the briefcase's own top-up log so it is observable without a separate audit query.

#### Scenario: Relevant top-up
- **WHEN** a request is within the tier/class policy and resolves to a relevant source
- **THEN** the kernel applies it with a matching source digest and records the decision

#### Scenario: Replayed top-up
- **WHEN** the same request ID is submitted after any prior decision
- **THEN** the request is rejected and no second decision is appended

