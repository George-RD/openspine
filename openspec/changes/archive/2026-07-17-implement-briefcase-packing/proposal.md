# Proposal: Kernel briefcase packing

## Dependencies
- `openspine-schemas` briefcase, visibility, and top-up types
- Existing grant binding, identity resolution, registry, and Store
- Canon AD-021, AD-031, AD-032, and AD-121

## Problem
Tasks currently have no kernel-owned context blackboard. Context selection, worker visibility, and additional-context requests therefore lack one deterministic, digest-bound mediation point.

## Proposed Solution
Add kernel briefcase orchestration at the Grant→Run boundary. The kernel derives a stable semantic grant projection, selects relevant preferences and skills from a source snapshot, resolves the truthful lane counterparty, persists the blackboard by grant id, and records worker visibility. Top-up requests are evaluated by kernel policy, resolved only from relevant kernel-owned sources, digest-bound, and atomically applied with replay protection.

## Acceptance Criteria
- Identical task shape and source snapshot produce byte-identical packs.
- Every persisted task has a kernel-owned briefcase before worker spawn.
- Kernel-bound content is absent from worker views; scratch content cannot be returned output.
- Depth is deterministic from relationship tier × task class.
- Top-up decisions are source-digest-bound, relevance-filtered, gate-visible, and replay-protected.
- Worker visibility records are persisted per grant and worker.

## Out of Scope
- Mining or learning the depth table.
- New preference/skill artifact kinds or a worker HTTP protocol.
- Creating new identity records or automatic identity binding for genuinely unknown email counterparties; existing identity-store lookup is in scope.
- Producing `ReturnedOutput` sections in this change; the first worker-side producer lands with `implement-worker-runtime`. The `/v1/briefcase/export` endpoint is deferred to that change. The schema-level negative visibility guard is tested here; the kernel-mediated producer is not.
