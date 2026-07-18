# Proposal: Implement the skill artifact class

## Why

Skills are a versioned artifact class that shapes an agent's competence, never
its authority (AD-040). Today the runtime has no first-class skill type: the
D-048 decision deliberately kept prompt templates fixture-only because an
instruction surface is an injection-escalation vector. AD-040's honest ground
for allowing runtime skills is the *gate-containment guarantee* — the gate
mediates every action regardless of what surface suggested it — which D-048
predated. This change introduces the artifact class, its provenance-gated
ceremony, the AD-110 promotion review for mined skills, and the read-only
matcher, all under test.

## What Changes

- Add the `Skill` schema (`openspine_schemas::skill`): `SkillProvenance`
  (ShippedSeed / UserInstalled / MinerDistilled), `SkillState`, versioned
  `Skill` with an opaque `body: String`, `task_shape` index keys, and
  per-agent/per-pack `SkillVisibility`. The type carries NO authority fields
  and uses `deny_unknown_fields` (structural containment, AD-040).
- Add the `skills` SQLite table and `store::skill_store` (separate from the
  `artifact.propose` pipeline per D-048), with provenance-branching insert and
  a digest-bound promotion transition that consumes an unforgeable
  `SkillReviewPassed` token.
- Add the install/update ceremony (`skill::ceremony`): trusted provenance
  commits straight to `Installed`; mined provenance lands `PendingReview` and
  is promoted only through the AD-110 review.
- Add the AD-110 promotion pass (`skill::review`): a deterministic first-cut
  evaluator that scans a mined skill body for authority-shaped keys and
  exfiltration markers, records its verdict in the eval-verdict store, and
  returns a digest-bound, unforgeable token.
- Add the AD-042 matcher (`skill::selection`): deterministic task-shape index
  plus a deterministic token-overlap semantic fallback, selecting ONLY from
  the approved shelf; it can inject, never install.

## Acceptance Criteria

- A mined skill cannot reach the shelf without a passing, digest-bound AD-110
  review; a malicious mined skill is denied and its rejected verdict is
  queryable.
- Shipped-seed and user-installed skills install to `Installed` without
  review; use is silent.
- The matcher cannot install; a poisoned skill's exfiltration attempt dies at
  `gate()` and the denial is audited (containment test).
- Rust formatting, lint, tests, file-size, claims, and strict OpenSpec
  validation are green.
