# Spec: Kernel registries

## Purpose

Define the kernel's compiled-in extension points — connectors, allowed-action dispatch, proposable artifact kinds, and the canonical set of known action ids — as explicit registries with fail-fast handling of unknown action ids, so that extending the kernel is a registration at a declared point rather than a match-arm edit scattered across the codebase.

## ADDED Requirements

### Requirement: Connectors MUST be registered through a connector registry

Connectors MUST be held in a single connector registry that is the one registration point for connector instances, and the registry MUST preserve per-connector absence: a connector that is not configured MUST be observably absent so callers keep their graceful-degradation behavior.

#### Scenario: Gmail is not configured

Given a kernel started without a Gmail configuration
When a flow that needs the Gmail connector runs (draft creation approval or `/draft` thread selection)
Then the connector registry MUST report the Gmail connector as absent
And the flow MUST degrade exactly as before: audit the absence and notify rather than fail.

#### Scenario: Connectors are enumerable

Given a kernel with configured connectors
When the connector registry is enumerated
Then every configured connector MUST appear with its registered name.

### Requirement: Allowed-action dispatch MUST resolve through a handler registry

Dispatch of a gate-allowed action MUST resolve its handler through an action-handler registry rather than a hardcoded match. An allowed action id with no registered handler MUST return the honest stub response, never an error. Approval-gated action ids (`email.create_draft`, `artifact.activate`) MUST NOT be directly dispatchable and MUST NOT have registered dispatch handlers.

#### Scenario: A known action without a kernel implementation is dispatched

Given a grant allows an action id that has no registered handler
When the action is dispatched after a gate Allow
Then the kernel MUST return the stub response identifying the action as unimplemented
And MUST NOT return an error status.

#### Scenario: An approval-gated action cannot be dispatched directly

Given a shell submits `email.create_draft` or `artifact.activate` to the actions endpoint
When gate evaluation and dispatch run
Then the action MUST NOT execute via a dispatch handler
And the only path to its effect MUST remain the digest-bound approval callback.

### Requirement: Post-approval resolution MUST route through a registry with a draft-creation default

Resolution of an approved action request MUST look up its handler by action id in a registry whose default entry routes to draft creation, preserving the invariant that every approval minted before artifact activation existed resolves as a draft.

#### Scenario: A non-activation approval resolves as a draft

Given an approved action request whose action id is not `artifact.activate`
When the approval callback resolves it
Then the kernel MUST route it to draft creation exactly as before.

### Requirement: Proposable artifact kinds MUST have a single source of truth

The set of proposable artifact kinds and each kind's parsing, duplicate-check, and overlay-layout behavior MUST derive from one registry table. Prompt templates MUST NOT appear in the table.

#### Scenario: A kind is validated and parsed

Given a chat proposes an artifact of kind `route | agent | workflow | pack | policy`
When `artifact.propose` validates, parses, and duplicate-checks it
Then all three steps MUST consult the same kind table.

#### Scenario: Templates remain non-proposable

Given a chat proposes an artifact of kind `template`
When `artifact.propose` is dispatched
Then the kernel MUST return a bad-request error.

### Requirement: Unknown action ids MUST fail fast at composition

A canonical catalog of known action ids MUST exist, and authority composition MUST reject any candidate action id absent from the catalog with a structured error naming the id, minting no grant. Known-but-unimplemented ids MUST remain composable.

#### Scenario: A fixture references an unknown action id

Given an agent, workflow, pack, or policy artifact carries an action id absent from the canonical catalog
When authority composition runs
Then composition MUST fail with a structured error naming the unknown id
And MUST NOT mint a task grant.

#### Scenario: An unwired but known id composes

Given a pack lists `route.activate` (known, intentionally unimplemented)
When authority composition runs
Then composition MUST succeed exactly as today.

### Requirement: Unknown action ids MUST be denied at gate with a structured reason

`gate()` MUST deny a requested action id absent from the canonical catalog with a denial reason distinct from the not-granted reason, and the denial MUST be audited like every other gate denial.

#### Scenario: The shell requests an unknown action id

Given a shell submits an action id absent from the canonical catalog
When gate() evaluates the request
Then the decision MUST be a denial with the unknown-action reason
And an audit event MUST record it.

#### Scenario: A known ungranted action keeps the not-granted denial

Given a shell submits a catalog-known action id absent from its grant's lists
When gate() evaluates the request
Then the decision MUST remain the existing not-granted denial.
