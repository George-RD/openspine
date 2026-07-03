# Spec: OpenSpine development process

## MODIFIED Requirements

### Requirement: OpenSpec artifacts MUST NOT be treated as live runtime authority

OpenSpec artifacts MUST NOT be treated as live runtime authority.

#### Scenario: Proposal describes a new capability pack

Given an OpenSpec proposal describes a new capability pack
When the proposal is created
Then the capability pack remains a proposed development artifact
And it MUST NOT become active runtime authority
And activation MUST require OpenSpine runtime artifact validation, approval, and lifecycle activation rules.

#### Scenario: Task file includes implementation steps

Given an OpenSpec `tasks.md` file lists implementation work
When an agent starts applying tasks
Then the task list guides development work only
And it MUST NOT grant broader filesystem, connector, model, memory, or network access inside OpenSpine runtime.

### Requirement: Completed OpenSpec changes MUST be archived

Completed OpenSpec changes MUST be archived after tasks are complete and specs are synced.

Archived changes MUST preserve:

- proposal rationale;
- design rationale;
- spec deltas;
- task history;
- decision-log changes where applicable.

#### Scenario: Change is complete

Given all tasks for a change are complete
When the change is accepted
Then the change SHOULD be archived under `openspec/changes/archive/`.

#### Scenario: Completed process change

Given all tasks for this change are complete
When the change is archived
Then its artifacts SHOULD remain available under `openspec/changes/archive/YYYY-MM-DD-<change-id>/`.

## ADDED Requirements

### Requirement: Security-load-bearing subsystems MUST gain a capability spec in the change that implements them

A change implementing a security-load-bearing subsystem MUST add that
subsystem's capability spec in the same change, not defer it to a later
backfill. Such subsystems include authority, approval, budgets, audit,
containment, connectors, and the model gateway.

#### Scenario: A change implements a new gated subsystem

Given a change implements a new security-load-bearing subsystem
When the change's tasks are planned
Then the plan MUST include adding that subsystem's capability spec
And the spec MUST land in the same change as the implementation, not a
separate later change.
