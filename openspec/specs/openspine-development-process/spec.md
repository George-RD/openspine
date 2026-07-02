# Spec: OpenSpine development process

## Purpose

Define the canonical OpenSpec process for turning the OpenSpine PRD and decision log into scoped implementation changes, specs, designs, and tasks — as the development/change-management layer, without confusing OpenSpec artifacts with OpenSpine's own runtime authority model.

## Requirements

### Requirement: OpenSpec development process MUST define its purpose

The OpenSpine development process MUST define how OpenSpec is used to develop OpenSpine.

OpenSpec MUST be treated as the development/change-management layer.

OpenSpine MUST be treated as the runtime substrate.

OpenSpec artifacts MUST NOT grant runtime authority inside OpenSpine.

#### Scenario: Development process is documented

Given the OpenSpine repository uses OpenSpec
When a development-process change is created
Then the change MUST explain how OpenSpec is used to develop OpenSpine
And it MUST distinguish OpenSpec development artifacts from OpenSpine runtime artifacts.

### Requirement: OpenSpec artifacts MUST NOT be treated as live runtime authority

OpenSpec artifacts MUST NOT be treated as live runtime authority.

#### Scenario: Proposal describes a new capability pack

Given an OpenSpec proposal describes a new capability pack
When the proposal is created
Then the capability pack remains a proposed development artifact
And it MUST NOT become active runtime authority
And activation MUST require OpenSpine runtime artifact validation, approval, and lifecycle activation rules.

### Requirement: Each OpenSpec change MUST state affected layer

Each OpenSpec change MUST state whether it affects OpenSpine core, Lyra product, both, or development tooling.

#### Scenario: Runtime substrate change is proposed

Given a change modifies task grants, authority composition, gate(), route resolution, connectors, model gateway, audit, or containment
When the proposal is written
Then it MUST classify itself as affecting OpenSpine core.

### Requirement: Authority-sensitive changes MUST be explicitly marked

A change MUST be marked authority-sensitive if it affects runtime authority, private data, external communication, connector access, account roles, model access, audit, containment, or system operations.

#### Scenario: Connector change is proposed

Given a change proposes adding a connector
When the proposal is created
Then it MUST be marked authority-sensitive
And it MUST describe connector trust posture, account role, event authenticity, and allowed/denied actions.

### Requirement: Security-sensitive changes MUST include verification tasks

A change affecting private data, external communication, containment, prompt-injection boundaries, audit, model gateway, approval, or secrets MUST include verification tasks.

#### Scenario: Model gateway behavior changes

Given a change modifies model gateway behavior
When tasks are created
Then tasks MUST include verification that private-context model calls go through the model gateway
And tasks MUST include verification that external content is wrapped as untrusted data.

### Requirement: Decision-log consistency MUST be preserved

Before changing architecture, terminology, scope, or authority semantics, the implementer MUST check the decision log.

#### Scenario: Proposal conflicts with accepted decision

Given a proposal conflicts with an accepted decision
When the proposal is reviewed
Then the change MUST identify the conflict
And it MUST include a new decision-log entry if accepted.

### Requirement: PRD-derived work MUST be split into implementation slices

The PRD MUST NOT be implemented as one large change.

#### Scenario: User asks to build OpenSpine generally

Given the user asks to build OpenSpine generally
When OpenSpec work is created
Then the work MUST be split into small implementation slices.

### Requirement: Completed OpenSpec changes MUST be archived

Completed OpenSpec changes MUST be archived after tasks are complete and specs are synced.

#### Scenario: Change is complete

Given all tasks for a change are complete
When the change is accepted
Then the change SHOULD be archived under `openspec/changes/archive/`.

### Requirement: Tool-specific skills MUST avoid unintentional drift

OpenSpec skills and commands for Claude, Codex, and OpenCode MUST avoid unintentional behavioral drift.

#### Scenario: One tool skill changes

Given one tool-specific OpenSpec skill is changed
When equivalent tool skills exist
Then the change SHOULD update the equivalent skills
Or explain why divergence is intentional.
