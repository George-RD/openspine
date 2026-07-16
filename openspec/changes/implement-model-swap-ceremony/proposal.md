# Proposal: Model swap ceremony

## Dependencies
- `implement-overlay-eval-gate` (archived): digest-bound AD-142 promotion and eval-verdict store.
- AD-152, AD-142, D-004, D-011, D-046, D-050, D-055, D-056, D-060.

## Problem
The gateway currently selects its provider from startup configuration, while AD-152 requires every base, matcher, and miner model swap to be an evidence-bearing owner-approved proposal. A runtime swap must not be possible without the ceremony, and a config/provider change must not bypass it for the governed active-role surface.

## Context
Golden-set cases are kernel/operator-owned immutable inputs. A proposer may select a role, an already configured provider, and a trusted golden-set id, but cannot supply cases or pass/fail results. The kernel runs every bounded case against the candidate, derives deterministic criterion verdicts, and stores bounded observed evidence in the digest-bound proposal bytes.

## Proposed Solution
Add `model_swap` as a sixth authority-bearing proposal kind. Enrich it through a verified golden-set runner, route the enriched bytes through the AD-142 gate and existing atomic promotion, and activate only after digest-bound owner approval. Startup builds an immutable provider pool and fails closed if an active persisted swap references a missing or changed provider/golden set. The active role map is the only runtime-proposable model-selection surface; adding or editing provider credentials/configuration remains bootstrap-only and requires restart.

## Acceptance Criteria
- A model swap without kernel-generated golden-set evidence cannot reach `review_required`.
- Golden-set format, caps, role binding, and deterministic pass/fail criteria are specified.
- Approved activation changes the provider used by the real `/v1/model/generate` path.
- Activation and restart re-check golden-set and non-secret provider-config digests.
- Matcher and miner roles are representable even though only Base has a current consumer.
- Local Rust, file-size, and strict OpenSpec gates pass.

## Out of Scope
- Full AD-111 prover/verifier semantics or evaluator independence (D-056/D-060).
- Adding matcher/miner inference consumers; their role assignments are governed now for future consumers.
- Runtime addition of credentials or arbitrary provider endpoints.
