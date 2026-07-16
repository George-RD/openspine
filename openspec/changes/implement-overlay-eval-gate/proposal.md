# Proposal: Overlay evaluation gate

## Dependencies

- `define-lineage-and-eval-store` (archived): indexed verdict landing and lineage.
- Artifact lifecycle propose → approve → activate.

## Problem

Authority-bearing proposals can currently reach `review_required` without replay or risk-judge evidence.

## Context

AD-142 requires offline replay against captured owner history and an adversarial pass before the owner tap. D-056 leaves evaluator policy details open while settling the verdict landing surface.

## Proposed Solution

Add opaque replay/judge proofs and an atomic store promotion operation. Reject the generic validated→review-required mutation and direct review-required inserts. Persist both digest-bound verdicts before exposing the approval button. Fail closed when owner-control history is unavailable.

## Acceptance Criteria

- No authority-bearing proposal reaches the approval surface without two digest-bound verdicts.
- Evidence is persisted and included in the owner confirmation summary.
- Store boundary rejects bypass attempts.

## Out of Scope

Judge independence, evaluator identity, attack-trace semantics, and verdict vocabulary beyond D-056's open landing schema.
