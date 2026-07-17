# Proposal: Implement workflow state machines

## Why

Workflow manifests currently describe only a human-readable step list. The kernel needs a reviewable deterministic spine that can be rendered as a graph, distinguish agentic and deterministic work, place escalation and approval semantics on states, and route model effort by declared step tier. Approval-gated transitions must remain digest-bound and replay-safe.

## What Changes

- Extend `WorkflowManifest` with optional states, transitions, step kinds, escalation points, approval semantics, and reasoning tiers while preserving legacy manifests.
- Render declared transitions as Mermaid flowchart syntax and validate state/step references.
- Add a static tier-to-provider map consumed by the model gateway, with current active-provider fallback so approved model swaps are reflected.
- Add a `WorkflowStateMachine` wrapper over `WorkflowCtx` that durably records transitions, resumes its cursor, and verifies Store-backed D-011 approvals before leaving approval-semantic states.
- Add deterministic schema, gateway-routing, approval, and replay tests.
