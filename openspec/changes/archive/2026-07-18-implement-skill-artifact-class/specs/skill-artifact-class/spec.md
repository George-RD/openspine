# Skill artifact class

## ADDED Requirements

### Requirement: Skills are a versioned artifact class shaping competence only

A skill is a versioned how-to procedure (AD-040) that shapes an agent's
competence and MUST NOT carry authority. The `Skill` schema MUST NOT define
any `allowed_actions`, `approval_required_actions`, `denied_actions`, or
`allowed_egress_classes` field; it MUST use `deny_unknown_fields` so a skill
body containing authority-shaped JSON can only ever land in the opaque
`body: String`, never be parsed into a structured grant. Authority stays in
packs and `gate()`; the real guarantee is that `gate()` mediates every action
regardless of what surface (trusted, poisoned, or no skill) suggested it
(AD-040).

#### Scenario: Skill wire shape rejects authority fields

- **WHEN** a `Skill` JSON value carries an `allowed_actions` (or other authority-shaped) field
- **THEN** deserialization MUST fail with an unknown-field error, so the field can never enter a grant-shaped structure (`test: skill_wire_shape_rejects_authority_fields`)

#### Scenario: A poisoned skill cannot widen granted authority at the gate

- **WHEN** a malicious (user-installed) skill whose body embeds authority-shaped keys and an exfiltration instruction is on the shelf and an agent requests an action the governing pack denies
- **THEN** `gate()` MUST deny the action exactly as without the skill, and the denial MUST be audited (`test: containment_gate_denies_denied_action_regardless_of_malicious_skill`, `test: poisoned_skill_counterparty_denial_surfaces_via_escalation`)

### Requirement: Skill provenance gates the install/update ceremony

Skills enter the shelf through a separate, user-controlled install/update
ceremony (AD-041), distinct from the five-kind `artifact.propose` pipeline
(D-048). Install/update branches by provenance: `ShippedSeed` and
`UserInstalled` are already trusted (the install act was the approval) and
commit straight to `Installed`; `MinerDistilled` lands in `PendingReview` and
MUST clear the AD-110 promotion review before it is visible. The same entry
point serves first install and update (each content version is one owner
decision).

#### Scenario: Trusted provenance installs without review

- **WHEN** a `ShippedSeed` or `UserInstalled` skill is installed
- **THEN** it MUST be stored in `Installed` state without any promotion review (`test: trusted_provenance_installs_without_review`, `test: user_installed_provenance_installs_without_review`)

#### Scenario: Mined provenance lands pending and review denies a malicious body

- **WHEN** a `MinerDistilled` skill whose body carries an authority-shaped key or exfiltration instruction is installed and then promoted
- **THEN** it MUST be stored `PendingReview` on install, and promotion MUST deny it and leave it off the shelf (`test: mined_provenance_lands_pending_and_review_denies_malicious_body`)

### Requirement: Mined skills require AD-110 promotion review before the shelf

A `MinerDistilled` skill MUST NOT reach `Installed` except by consuming an
unforgeable `SkillReviewPassed` token produced by the AD-110 promotion pass
run against the exact skill bytes (digest-bound). The review verdict MUST be
persisted in the eval-verdict store (the review digest surface) before the
skill is promoted, so the owner can see why a skill was accepted or refused
without ever seeing the skill body (D-012).

#### Scenario: Mined skill promotes only after a passing review

- **WHEN** a benign `MinerDistilled` skill passes the promotion review
- **THEN** it MUST move to `Installed` and its `approved` verdict MUST be queryable from the eval-verdict store (`test: mined_provenance_promotes_when_review_passes`, `test: promotion_review_approved_verdict_is_queryable`)

#### Scenario: Rejected promotion verdict is queryable

- **WHEN** a `MinerDistilled` skill fails the promotion review
- **THEN** promotion MUST deny it, leave it off the shelf, and persist a `rejected` verdict (with the matched marker) in the eval-verdict store (`test: promotion_review_denial_verdict_is_queryable`)

#### Scenario: Owner can reject a pending mined skill

- **WHEN** the owner rejects a `PendingReview` mined skill
- **THEN** it MUST transition to `Rejected` (terminal) and stay off the shelf (`test: owner_can_reject_pending_mined_skill`)

### Requirement: Skill selection is a read-only matcher that injects, never installs

Skill selection (AD-042) is a deterministic index (task class → skills) plus a
deterministic semantic-matcher fallback, selecting ONLY from the approved
shelf. The matcher takes `&Store` and returns `Vec<Skill>`; it MUST NOT
install, mutate, or create any skill row. Visibility is scoped per agent OR
pack (deny-by-default): a skill must be visible to at least one of them to be
selected. The fallback ranks visible `Installed` candidates by token overlap
and returns only those sharing at least one token; unrelated task classes
match nothing.

#### Scenario: Matcher can inject but never install

- **WHEN** the matcher selects an installed skill for an agent's task shape
- **THEN** it MUST return the skill but MUST NOT create or modify any skill row (`test: matcher_can_inject_but_never_install`)

#### Scenario: Pack-scoped skill is visible only to pack members

- **WHEN** a skill is scoped only to a pack (not any specific agent)
- **THEN** it MUST be selectable by an agent composing a grant under that pack and invisible to agents outside it (`test: pack_scoped_skill_visible_only_to_pack_members`)

#### Scenario: Semantic fallback selects only installed, visible candidates

- **WHEN** no skill's task shape matches exactly and an unrelated task class is requested
- **THEN** the fallback MUST return only `Installed`, visible, token-overlapping skills (never a pending one) and MUST return nothing for an unrelated class (`test: semantic_fallback_selects_only_installed_visible`)

### Requirement: The owner promotion tap MUST be an authenticated, durable decision (AD-041/AD-110)

The owner promotion tap MUST be the ONLY owner entry point (`owner_decide_promotion`) that lands a mined skill on the shelf, and it MUST authenticate the caller with a genuine `VerifiedOwnerContext` (minted only by Telegram owner verification) AND MUST verify the supplied `owner_principal_id` resolves to the configured owner. On `Approve`
it delegates to the AD-110 evaluator (the sole issuer of the unforgeable
promotion token), so owner approval is necessary but never sufficient to bypass
the evaluator. The decision is durably persisted in `skill_promotion_decisions`
atomic with the shelf transition (AD-041: one decision per skill version, ever —
enforced by a `UNIQUE(skill_id, version)` constraint that fails a repeat tap
closed). The persisted `decision` records the OWNER's intent (`"approve"`) even
when the evaluator denies, so an approve-then-denied skill is never mislabeled
as an owner rejection; `result_state` records the actual shelf outcome
(`Rejected`).

#### Scenario: Owner approval routes through the AD-110 evaluator and persists a durable decision

- **WHEN** the owner taps Approve on a benign `PendingReview` mined skill
- **THEN** it MUST promote to `Installed` and a `skill_promotion_decisions` row (decision `approve`, owner principal, `Installed`) MUST be durably written (`test: owner_approval_promotes_through_evaluator`)

#### Scenario: Owner approve with an evaluator denial is labeled with owner intent, not a rejection

- **WHEN** the owner taps Approve on a poisoned mined skill that the AD-110 evaluator denies
- **THEN** the skill MUST stay `Rejected`, the persisted decision MUST be `approve` (owner intent) with `result_state=Rejected`, and exactly one decision row MUST exist (`test: owner_approve_but_evaluator_denies_labels_decision_approve`)

#### Scenario: A repeat owner tap on an already-decided skill version fails closed

- **WHEN** a second owner decision (approve or reject) targets a skill id+version that already has a persisted decision
- **THEN** it MUST fail closed and MUST NOT persist a contradictory second row (`test: repeat_owner_tap_on_same_version_fails_closed`)

### Requirement: Installing or promoting a version MUST atomically retire lower versions (AD-041)

Activating a skill version MUST atomically RETIRE every lower-version `Installed`
row for the same `id` (reusing `SkillState::Retired`), so a higher version
supersedes and hides stale lower versions; the shelf exposes at most the
highest active version per `id`. A version equal to or lower than an existing
(any-state) version is refused on trusted install, and a version no higher
than an already-`Installed` version is refused on promotion.

#### Scenario: Promoting a higher version retires the lower Installed version atomically

- **WHEN** a higher version of a skill is installed (trusted) or promoted (mined) while a lower version is `Installed`
- **THEN** the lower version MUST transition to `Retired` and the matcher MUST return only the higher version (`test: promotion_retires_lower_installed_version_atomically`, `test: trusted_install_refuses_lower_version_when_higher_exists_any_state`, `test: promotion_refuses_when_higher_installed_exists`)

### Requirement: `skill.context` is a gated kernel action returning an untrusted envelope (AD-040/AD-042)

`skill.context` is a real, gated kernel action (not a fixture). It selects the
installed, approved-shelf skills matching the authenticated grant's agent/pack
and job purpose, and returns their `body` text inside an explicit
`untrusted: true` envelope the shell MUST treat as competence data, not
instructions. The agent/pack scope is derived from the authenticated `TaskGrant`
(its bound agent/pack), never from the caller's payload, so a grantee can only
ever query the skills its own grant's scope permits. The skill `body` is opaque
competence text; returning it confers no authority — `gate()` still mediates
every outbound effect the shell later chooses.

#### Scenario: skill.context selects only grant-scoped installed matches and marks them untrusted

- **WHEN** a grant holding `skill.context` reaches dispatch for a job purpose with an installed, grant-visible skill
- **THEN** the response MUST contain that skill's `body` inside an `untrusted: true` envelope scoped to the grant's agent/pack, and the caller MUST NOT be able to select a different task family (`test: skill_context_selects_only_grant_scoped_installed_matches`)
