# OpenSpine conventions

## Purpose

This file records cross-cutting conventions for developing OpenSpine with OpenSpec and Conflux.

## Naming

- Use `OpenSpine` for the reusable governed agent runtime substrate.
- Use `Lyra` for the first governed personal assistant product built on OpenSpine.
- Do not describe OpenSpine as “the Lyra agent.”
- Do not describe Lyra as the whole runtime substrate.

## OpenSpec boundary

- OpenSpec is the development/change-management layer.
- OpenSpec artifacts do not grant OpenSpine runtime authority.
- OpenSpec changes describe proposed work until applied and accepted.
- Runtime authority remains controlled by OpenSpine task grants, policy, and gate-mediated actions.

## Change structure

Each change SHOULD contain:

- `proposal.md`
- `design.md` when architecture, authority, security, or non-trivial design is involved
- `tasks.md`
- `specs/<area>/spec.md` delta specs

Each proposal SHOULD include:

- `## Dependencies`
- `## Problem/Context`
- `## Proposed Solution`
- `## Acceptance Criteria`
- `## Out of Scope`

## Requirement language

- Requirements use MUST, SHALL, SHOULD, or MAY.
- Authority and security requirements SHOULD normally use MUST.
- Each requirement includes at least one `#### Scenario:` block.

## Authority-sensitive changes

A change is authority-sensitive if it affects:

- source verification
- identity resolution
- route resolution
- route conflict handling
- capability packs
- task grants
- gate-mediated actions
- approval requirements
- connector access
- account roles
- memory scope
- external writes
- model provider access
- system operations
- audit or recovery
- containment

Authority-sensitive changes must include explicit verification tasks.

## Verification

Until implementation commands exist, use:

```bash
openspec validate --changes <change-id> --strict
```

When source code exists, update each change’s `tasks.md` with actual detected build, lint, typecheck, and test commands.
