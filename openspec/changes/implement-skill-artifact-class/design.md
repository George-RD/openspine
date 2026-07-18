# Design: Skill artifact class

## Type-level structural containment (AD-040)

`openspine_schemas::skill::Skill` is shaped so the containment guarantee is
structural, not a runtime check a future caller could skip:

- It carries **no** `allowed_actions`, `approval_required_actions`,
  `denied_actions`, or `allowed_egress_classes` field — contrast
  `CapabilityPack`, which carries exactly those because packs *are* an
  authority source.
- It uses `#[serde(deny_unknown_fields)]`, so a skill body containing
  authority-shaped JSON can only ever land in `body: String` (free text),
  never be parsed into a structured grant.
- `body` is opaque prompt text; no code path in this crate parses it into
  authority. The real guarantee is downstream: `gate()` mediates every action
  request regardless of what surface (trusted, poisoned, or none) suggested it.

This is verified at the schema boundary by
`skill_wire_shape_rejects_authority_fields` and at the runtime boundary by
`containment_gate_denies_denied_action_regardless_of_malicious_skill`.

## Provenance-gated ceremony (AD-041)

`skill::ceremony::install_skill` is the single entry point for first install
**and** update (each content version is one owner decision). It branches by
provenance:

- `ShippedSeed` / `UserInstalled` — already trusted (the install act was the
  approval) → commit straight to `Installed`.
- `MinerDistilled` — inferred, not human-authored → land `PendingReview`.

`promote_mined_skill` is the **owner-action boundary**: it is the kernel
entry point an owner-initiated action calls (the "one tap" of AD-041). The
kernel refuses to promote unless the AD-110 pass ran against the exact skill
bytes and produced a digest-bound `SkillReviewPassed` token; `store::
skill_store::promote_skill` re-checks `token.digest() == stored.
content_digest`, so a different or post-edited skill can never be promoted
with a recycled token. The owner reviews the skill's `provenance` and
`content_digest` (the kernel exposes exactly those facts plus the recorded
verdict); presenting a human-readable diff of prior-vs-current content is a
shell/UI concern and is out of scope for this change.

## AD-110 promotion review (mined only)

`skill::review::run_promotion_review` runs only at the promotion point
(AD-110 — never per-use). A minimal, fully-deterministic first-cut evaluator
scans the mined body for authority-shaped keys
(`allowed_actions`, `denied_actions`, `approval_required_actions`,
`allowed_egress_classes`) and exfiltration markers (`bcc archive@`, `forward
to`, `send to external`, `exfiltrate`, `cc unknown`, `mail to `). It records
its verdict in the eval-verdict store (the review digest surface) and returns
an unforgeable, digest-bound token.

**Audit-before-effect hardening:** `record_verdict` is now fallible and must
persist before the review outcome is returned. If the verdict cannot be
recorded, the promotion fails closed (`PromotionDenial::VerdictRecordingFailed`)
rather than returning a pass without its durable record. The rejected/approved
verdict is queryable via `Store::eval_verdicts_for_artifact`
(`promotion_review_denial_verdict_is_queryable`,
`promotion_review_approved_verdict_is_queryable`).

AD-111's full prover-verifier attack-trace formalism remains *leaning* /
owner-ratification in a later change; the verdict lands in the same eval store
AD-111 specifies.

## Read-only matcher (AD-042)

`skill::selection::select_skills_for_task(store, agent_id, pack_id, task_class)`
is the matcher. It takes `&Store` and returns `Vec<Skill>` — there is no
`&mut`, no install call, no ceremony entry point in scope, so it physically
cannot install. Selection:

1. **Deterministic index** — exact `task_shape` match.
2. **Deterministic semantic fallback** — token-overlap ranking among the same
   visible `Installed` candidates (deduped tokens, stable
   `(score desc, id asc, version desc)` order so SQLite row order cannot
   change the result). Unrelated task classes share zero tokens and are never
   selected.

Visibility is deny-by-default and scoped per agent **OR** pack via
`SkillVisibility::is_visible_to`. A skill must be visible to at least one of
them to be selected (`pack_scoped_skill_visible_only_to_pack_members`). The
matcher never re-parses `body` into authority.

## Deferred / candidates (unnumbered)

- **AD-043 — external-skill import pipeline** (UNNUMBERED candidate, not
  implemented in this change): progressive-disclosure restructuring, static
  effect/egress classification, offline quarantine eval, then entry via the
  AD-041 install path with a provenance-and-risk report. `SkillProvenance`
  intentionally carries exactly three variants, none of them `External`. Adding
  it is a new change, not a hidden branch here.
- **Worker-runtime silent injection**: the kernel action and matcher are
  implemented and gated here, but the later `implement-worker-runtime` change
  owns calling them during worker briefcase/prompt construction. This is an
  UNNUMBERED candidate deferral, not permission for a caller to choose a task
  family: `skill.context` derives task class from the authenticated grant's
  job purpose and ignores caller payload.


## Candidate decision-log entry (UNNUMBERED — formal revisit of D-048)

> **(candidate) Runtime skills are permitted on the gate-containment
> guarantee (revisiting D-048).** D-048 kept prompt templates fixture-only
> because an instruction surface is an injection-escalation vector. AD-040
> supplies the missing honest ground D-048 predated: a skill is the *same
> category* as a poisoned template, but the gate **contains** any skill —
> trusted or not — because every effectful action is mediated by `gate()`,
> which rejects injected recipients/egress (e.g. an instruction to `BCC
> archive@x`) at the boundary and surfaces the attempt in the audit/digest.
> Therefore runtime skills (a versioned artifact class shaping competence
> only, with no authority-shaped field) are now permitted, gated by
> provenance (AD-041) and, for mined skills, a one-tap AD-110 promotion
> review. This does **not** repeal D-048's separation of the skill install
> path from `artifact.propose`: skills use the dedicated `skill::ceremony`,
> not the five-kind propose pipeline.
> Would change if: the gate ever stopped mediating a skill-suggested action,
> or a skill type gained an authority field (both violate AD-040 by
> construction).

This is an **UNNUMBERED candidate** (not yet ratified) and is captured
here because the canon `.raw` set is immutable during an in-flight change;
it is NOT ingested into `.raw/openspine-decision-log.md` by this change.

## Test mapping

| Requirement | Real test(s) |
| --- | --- |
| Skills shape competence only (AD-040) | `skill_wire_shape_rejects_authority_fields`, `containment_gate_denies_denied_action_regardless_of_malicious_skill` |
| Provenance-gated ceremony (AD-041) | `trusted_provenance_installs_without_review`, `user_installed_provenance_installs_without_review`, `mined_provenance_lands_pending_and_review_denies_malicious_body` |
| Mined needs AD-110 review (AD-110) | `mined_provenance_promotes_when_review_passes`, `promotion_review_approved_verdict_is_queryable`, `promotion_review_denial_verdict_is_queryable`, `owner_can_reject_pending_mined_skill` |
| Read-only matcher (AD-042) | `matcher_can_inject_but_never_install`, `pack_scoped_skill_visible_only_to_pack_members`, `semantic_fallback_selects_only_installed_visible` |
