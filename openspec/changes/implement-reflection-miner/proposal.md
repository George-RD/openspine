# Proposal: Implement the reflection miner

## Dependencies

- `briefcase-packing` provides deterministic scoped worker context.
- `worker-runtime` provides ordinary task grants and gateway/gate execution.
- `overlay-eval-gate` and `overlay-model` provide proposal evaluation and learned-artifact provenance.
- AD-149, AD-050 (scheduled tier), AD-053, AD-054, AD-022, and D-096 are settled canon.

## Problem/Context

OpenSpine needs a periodic systemic reflection worker that can learn from a bounded audit backlog without becoming a privileged background daemon or silently mutating kernel authority. The worker must distinguish corrections, repeated approvals, and stated preferences; retain encrypted source provenance; and use positive instruction rewrites while keeping negative constraints as executable probes.

## Proposed Solution

Add a pure reflection-miner worker boundary in the schemas crate. Kernel admission accepts only an ordinary, empty-egress grant with gateway/gate-authorized model calls, pack-derived classification ceiling, artifact/model limits, and an exact scoped audit slice. The miner returns only lifecycle-proposed artifacts carrying source-event and encrypted-exchange provenance. Correction observations become positive instruction rewrites plus optional eval probes; repeated approvals become standing-rule candidates; preferences become preference artifacts; and consolidation emits a reviewable merge/prune proposal. Add the persona artifact kind to the normal proposal parser and dispatch path for the AD-135 digest default route.

The kernel remains responsible for authenticated grant admission, scheduled invocation, normal artifact persistence/approval/activation, and ProducedBy provenance from the submitting grant. Dynamic probe registration in the golden-set evaluator remains the explicit D-096 follow-up; this change emits the structured probe at the miner boundary without embedding negative text in persona guidance.

## Acceptance Criteria

- The miner has no kernel store or activation mutator and cannot write kernel state.
- Every reflection proposal carries source event and encrypted exchange provenance.
- A correction produces an instruction rewrite, never a prohibition append; negative constraints become `EvalProbe` data.
- Empty output channels, model/artifact limits, gateway/gate admission, pack classification ceilings, exact briefcase scope, and direct-mutation denial are enforced.
- Persona corrections enter the normal `artifact.propose` lifecycle as reviewable proposed rows.
- Repeated approvals and consolidation never quietly activate standing rules or learned artifacts.
