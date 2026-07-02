# Proposal: Define OpenSpine development process

## Summary

Define the canonical OpenSpec process for developing OpenSpine.

This change establishes how future agents and developers should turn the OpenSpine PRD and decision log into scoped implementation changes, specs, designs, and tasks.

The main goal is to make OpenSpec useful as the development/change-management layer without confusing it with OpenSpine runtime authority.

## What Changes

This change adds a canonical OpenSpine development-process specification.

It creates:

- a delta spec for the OpenSpine development process;
- a design explaining the OpenSpec/OpenSpine boundary;
- a task list for installing and reviewing the process artifacts;
- OpenSpine-specific OpenSpec configuration guidance;
- a recommended backlog of future implementation slices.

It does not implement OpenSpine runtime code.

## Why

The repository currently contains:

- OpenSpec skills and commands for multiple AI coding tools.
- A PRD describing OpenSpine as a governed runtime substrate.
- A decision log explaining why OpenSpine and Lyra are separate.
- A decision that OpenSpec is the development/change-management layer, not the runtime.

However, the repo does not yet define a canonical OpenSpine-specific OpenSpec process.

Without that process, future agents may:

- treat OpenSpec proposal/design/task files as if they grant runtime authority;
- make broad implementation changes directly from the PRD;
- skip decision-log updates when changing architecture;
- confuse OpenSpine core with Lyra product behavior;
- implement authority-sensitive behavior without testable requirements;
- let duplicated tool-specific skills drift across Claude, Codex, and OpenCode.

## Goals

- Define how OpenSpec should be used to develop OpenSpine.
- Establish OpenSpine-specific rules for proposals, specs, designs, and tasks.
- Require clear separation between OpenSpine substrate changes and Lyra product changes.
- Require extra care for changes affecting authority, private data, external communication, connectors, system operations, audit, containment, and model gateway behavior.
- Create a repeatable process for future implementation slices.

## Non-goals

- Do not implement OpenSpine runtime code in this change.
- Do not implement Telegram owner control.
- Do not implement Gmail or Google Workspace integration.
- Do not implement gate(), task grants, model gateway, containment, or audit.
- Do not create a custom OpenSpec schema yet.
- Do not archive this change until the process docs are reviewed and accepted.

## Scope

This change creates the initial OpenSpec documentation for OpenSpine development.

In scope:

- A canonical development-process spec.
- A design explaining how OpenSpec maps to OpenSpine development.
- Tasks for adding the process to the repo.
- Optional strengthening of `openspec/config.yaml`.

Out of scope:

- Runtime implementation.
- Tool-specific skill regeneration.
- OpenSpec schema customization.
- CI enforcement.
- Graphify integration beyond existing repo guidance.

## OpenSpine / Lyra boundary

OpenSpine is the reusable substrate.

Lyra is the first product built on OpenSpine.

Future OpenSpec changes MUST state whether they affect:

- OpenSpine core only;
- Lyra product only;
- both OpenSpine and Lyra.

## OpenSpec / OpenSpine boundary

OpenSpec governs development changes.

OpenSpine governs runtime behavior.

OpenSpec artifacts MAY describe, propose, plan, and track changes to OpenSpine.

OpenSpec artifacts MUST NOT be treated as live runtime authority.

Runtime authority remains controlled by OpenSpine concepts such as:

- verified events;
- deterministic routes;
- policy;
- agent manifests;
- workflows;
- capability packs;
- authority composition;
- task grants;
- gate-mediated actions.

## Risks

| Risk | Mitigation |
|---|---|
| Agents treat OpenSpec artifacts as runtime authority | Add explicit spec requirements forbidding this |
| PRD gets implemented as one oversized task | Require small scoped implementation slices |
| Decision log drifts from specs | Require decision-log update tasks for architecture changes |
| OpenSpine and Lyra naming blurs again | Require every proposal to classify affected layer |
| Security-sensitive changes lack verification | Require explicit verification tasks |
| Tool-specific skills drift | Add future task to define canonical skill source or drift check |

## Proposed first implementation slices after this change

Recommended next changes:

1. `define-core-runtime-schemas`
2. `implement-task-grant-composer`
3. `implement-gate-action-api`
4. `implement-telegram-owner-control-slice`
5. `implement-selected-thread-email-preview-slice`

These should be separate OpenSpec changes, not bundled into this process change.
