# Spec: OpenSpine development process

## ADDED Requirements

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


### Requirement: OpenSpec MUST remain separate from OpenSpine runtime authority

OpenSpec artifacts MUST NOT be treated as live runtime authority.

OpenSpec artifacts MAY describe proposed routes, workflows, agents, capability packs, policies, or task grants.

OpenSpec artifacts MUST NOT directly activate routes, workflows, agents, capability packs, policies, or task grants in a running OpenSpine system.

Runtime authority MUST be derived through OpenSpine’s own runtime authority model.

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

### Requirement: Every change MUST classify its affected layer

Each OpenSpec change MUST state whether it affects:

- OpenSpine core;
- Lyra product;
- both OpenSpine and Lyra;
- development tooling only.

The classification MUST appear in the proposal.

#### Scenario: Change affects runtime substrate

Given a change modifies task grants, authority composition, gate(), route resolution, connectors, model gateway, audit, or containment
When the proposal is written
Then it MUST classify itself as affecting OpenSpine core.

#### Scenario: Change affects Telegram assistant UX

Given a change modifies Telegram owner-control behavior, Lyra assistant wording, or selected-thread email user experience
When the proposal is written
Then it MUST classify itself as affecting Lyra product.

### Requirement: Authority-sensitive changes MUST be explicitly marked

A change MUST be marked authority-sensitive if it affects any of the following:

- event source verification;
- identity resolution;
- route resolution;
- route conflict handling;
- capability packs;
- task grants;
- gate-mediated actions;
- approval requirements;
- connector access;
- account roles;
- memory scope;
- external writes;
- model provider access;
- system operations;
- audit or recovery;
- containment.

Authority-sensitive changes MUST include explicit verification tasks.

#### Scenario: Change broadens email access

Given a change proposes inbox-wide email reads
When the proposal is created
Then it MUST be marked authority-sensitive
And the design MUST describe why selected-thread limits are being changed
And tasks MUST include tests proving denied access remains denied outside the new scope.

#### Scenario: Change adds a new connector

Given a change proposes adding Slack, Outlook, WhatsApp, GitHub, Coolify, or another connector
When the proposal is created
Then it MUST be marked authority-sensitive
And it MUST describe connector trust posture, account role, event authenticity, and allowed/denied actions.

### Requirement: Security-sensitive changes MUST include verification tasks

A change affecting private data, external communication, containment, prompt-injection boundaries, audit, model gateway, approval, or secrets MUST include verification tasks.

Verification MAY include tests, review checklists, threat scenarios, or executable checks depending on implementation maturity.

#### Scenario: Change touches model gateway

Given a change modifies model gateway behavior
When tasks are created
Then tasks MUST include verification that private-context model calls go through the model gateway
And tasks MUST include verification that external content is wrapped as untrusted data.

#### Scenario: Change touches approval flow

Given a change modifies approval behavior
When tasks are created
Then tasks MUST include verification that approval is bound to the exact payload and target digest.

### Requirement: Decision-log consistency MUST be preserved

Before changing architecture, terminology, scope, or authority semantics, the implementer MUST check the decision log.

If a change reverses, weakens, or materially refines an accepted decision, the change MUST include a task to update the decision log.

#### Scenario: Change narrows OpenSpine into a single assistant app

Given a proposal describes OpenSpine as only a personal assistant app
When the proposal is reviewed
Then the change MUST identify that it conflicts with the OpenSpine/Lyra separation decision
And it MUST include a new decision-log entry if accepted.

#### Scenario: Change keeps existing decisions intact

Given a proposal implements a phase already described in the PRD
When it does not reverse or weaken accepted decisions
Then it SHOULD cite the relevant decision but does not need a new decision entry.

### Requirement: PRD-derived work MUST be split into implementation slices

The PRD MUST NOT be implemented as one large change.

Future OpenSpec changes SHOULD map to small, independently reviewable implementation slices.

Recommended initial slices:

- core runtime schemas;
- authority composition and task grants;
- gate-mediated action API;
- Telegram owner-control slice;
- selected-thread email preview slice;
- digest-bound draft approval slice.

#### Scenario: Agent proposes “build OpenSpine”

Given the user asks to build OpenSpine generally
When the agent creates OpenSpec work
Then the agent SHOULD propose a small first implementation slice
And SHOULD NOT create one monolithic change for the whole PRD.

### Requirement: OpenSpec archive MUST preserve rationale

Completed OpenSpec changes MUST be archived after tasks are complete and specs are synced.

Archived changes MUST preserve:

- proposal rationale;
- design rationale;
- spec deltas;
- task history;
- decision-log changes where applicable.

#### Scenario: Completed process change

Given all tasks for this change are complete
When the change is archived
Then its artifacts SHOULD remain available under `openspec/changes/archive/YYYY-MM-DD-define-openspine-development-process/`.

### Requirement: Tool-specific skills MUST avoid unintentional drift

OpenSpec skills and commands for Claude, Codex, and OpenCode MUST avoid unintentional behavioral drift.

Future changes to OpenSpec workflow behavior MUST either:

- update all tool-specific copies; or
- introduce a canonical source and generation process.

#### Scenario: Claude skill is updated

Given `.claude/skills/openspec-propose/SKILL.md` is changed
When equivalent Codex and OpenCode skills exist
Then the change SHOULD either update the equivalent skills
Or explain why divergence is intentional.
