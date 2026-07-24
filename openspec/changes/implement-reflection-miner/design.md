# Design: Reflection miner worker boundary

## Authority and scheduling

The miner is a worker role, not a daemon. The kernel invokes it from the scheduled internal lane under an ordinary authenticated task grant. The schemas boundary assumes the kernel has already verified the grant MAC and expiry, then enforces miner-specific restrictions: no output channels, model generation only, no activation/policy/standing-rule mutation actions, pack-derived classification ceiling, and bounded model/artifact counts. Model requests are data passed to the existing gateway after gate admission; the miner performs no provider I/O.

## Scoped context and provenance

`MinerBriefcase` is a read-only audit slice keyed to the grant and scope. Every observation must match an entry's event ID and encrypted exchange reference exactly. `ReflectionProvenance` is mandatory on every observation and proposal. The owner-correction submitting grant in the AD-135 route is the normal lifecycle ProducedBy anchor; the integration test proves its event and exchange are identical to the miner observation source.

## Output classes and positive steering

- Corrections produce `InstructionRewrite { instruction, reason, eval_probe }`.
- Negative constraints are structured `EvalProbe` values, never persona guidance or prohibition artifacts.
- Repeated approvals produce `StandingRuleCandidate`, remaining `proposed` until normal owner review.
- Stated preferences produce preference/overlay proposals.
- Consolidation produces a proposed merge/prune operation; the kernel applies no mutation directly.

`ReflectionProposal` has no activation operation and always enters `Lifecycle::Proposed`. Persona payloads serialize to `PersonaElement` YAML, and the kernel artifact kind table accepts `persona` through the existing proposal/eval/persistence path.

## AD-135 persona route

The persona kind is added to `ParsedProposal`, its kind table, registry insertion, YAML serialization, activation state transition, and deterministic judge handling. A dispatch-level integration test runs `ReflectionMiner::mine` → `to_proposal_payload` → `dispatch_artifact_propose`, verifies the rewrite/probe/provenance, and asserts a persisted `proposed_artifacts` row. Activation continues to derive ProducedBy from the authenticated submitting grant, preserving the exact correction exchange binding.

## Explicit boundaries

Dynamic miner-generated probe registration in the golden-set evaluator is not added here: D-096 defines deterministic personality probes and records the correction route as this change's implementation boundary. This change emits first-class `EvalProbe` data and tests it; a later evaluator extension may bind those probes into a model-evaluation corpus without putting negative constraints into prompt artifacts.
