# Proposal: implement-personality-seed

## Dependencies

- `implement-overlay-model` (archived 2026-07-17): artifact-loader registry,
  learned-artifact provenance and namespace machinery (D-077..D-081).
- `implement-overlay-eval-gate` (archived 2026-07-16): the eval-harness probe
  surface that anti-pattern probes plug into.

## Problem/Context

AD-080 ships an opinionated eight-element Donna×Leo personality as
pre-populated, learnable overlay artifacts — day one Donna-ish, year one the
user's — explicitly *not* kernel-baked. AD-081 and AD-083 enumerate the
negative constraints (seven from AD-081; faked intimacy, info-dump without
synthesis, and self-promotional visibility added by AD-083) that must be
*testable* constraints, never prompt text (AD-054). AD-082 (digest/brief
triage) and AD-135 (ship defaults, learn preferences) settle the digest/brief
format as a learnable overlay default rather than a spec-fixed decision.

Today there is no persona artifact kind in the loader, and no anti-pattern
probe exists in the eval harness. Both gaps must close so the seed loads with
traceable provenance and the anti-patterns are enforceable as eval scenarios.

## Proposed Solution

- Add a **persona** artifact kind to the loader (a seventh kind, loaded exactly
  like `model_swap` — present in the registry, absent from the proposable-kind
  table because it carries no authority). Each of the eight AD-080 elements
  plus the AD-082 digest/brief default becomes its own `PersonaElement`
  overlay artifact.
- Seed the nine elements at kernel bootstrap into `data/artifacts.d/personas`
  as learnable overlay artifacts with genuine `ProducedBy` provenance
  (D-077), via the existing overlay machinery — never as base fixtures. The
  seed is idempotent: only elements missing from `learned_artifacts` are
  written, and each on-disk YAML is durable before its provenance row is
  recorded.
- Add ten deterministic anti-pattern probes (AD-081/AD-083) to the eval
  harness. Each returns a violation on output that exhibits the pattern and
  passes clean output; the harness fails any output-under-test that trips a
  probe.

## Acceptance Criteria

- Seed artifacts load as overlay learned artifacts carrying `ProducedBy`
  provenance and survive a kernel restart.
- Seed artifacts are never base/kernel-baked fixtures (per AD-080).
- Every AD-081 and AD-083 anti-pattern has an eval probe that fails on a
  violating sample and passes on a clean sample.

## Out of Scope

- Wiring `PersonaElement` guidance into the live prompt builder (a future
  change; `AgentManifest.persona` is the future reference hook).
- Proposing persona edits through chat. The seed is a kernel-authored,
  learnable default; the AD-053/AD-054 correction→miner→proposal loop is the
  future convergence path.
- Full AD-111 attack-trace semantics for the probes — these are minimal,
  deterministic first cuts (D-056), like `judge`/`replay`.
