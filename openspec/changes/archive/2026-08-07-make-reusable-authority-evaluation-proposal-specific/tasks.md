# Tasks

Every box below is ticked only where the work exists in the diff. Where the
original plan named work that turned out to be unbuildable or wrong, the box is
left unticked and the reason is recorded under it — a ticked box for absent work
is worse than an untouched one, because the next change builds on it.

## 1. Pin the current ceremony before replacing it

- [x] Regression tests characterise today's behaviour honestly, so the
      replacement is provably a change in kind and not a rename.
  - `judge_refuses_standing_rule_whose_action_has_no_registered_executor` pins
    that a scope-bound rule whose action cannot execute no longer reaches the
    owner.
  - `availability_only_evaluation_makes_no_replay_claim` pins that copy claiming
    executed cases requires a non-empty ledger. It caught a real overclaim during
    implementation: the renderer still emitted the literal label `(replay: pass)`
    on a proposal where nothing ran, and now names each evaluator from its own
    recorded evidence.

## 2. Assemble a kernel-derived evaluation input

- [x] `CanonicalEvaluationInput` in `overlay_eval_gate/eval_input.rs`, private
      fields, no public constructor, following the `ReplayPassed` containment
      pattern.
- [x] Assembled only from kernel sources: `ActionCatalog::delegation_descriptor_for`,
      `implementation_descriptor_for_action`, `is_non_delegable`, the readiness
      closure built in `artifact_propose.rs`, the composed `denied_actions` set,
      the active-rule list, and the epoch set from task 5.
- [x] Typed `IncompleteInput` denials, each naming the dimension:
      `MissingDimension`, `NoDelegationDescriptor`, `NoImplementationDescriptor`,
      `InconsistentScopeBinding`, `ScopeMissingRequiredDimensions`.
  - Tests: `incomplete_scope_binding_denies_by_dimension_rather_than_passing`,
    `inconsistent_scope_binding_is_refused_as_incomplete_input`.
- [ ] Bind the evidence-set digest. **Not done, and not buildable here:**
      `DelegationEvidence` exists only in `openspine-schemas`' *test* files, not
      in `src/`, so there is no production type to read an evidence set from. The
      epoch is recorded as `None` (`eval_input.rs:233`) rather than fabricated,
      and `VerdictEpochs` compares no axis a verdict did not record. Wiring it is
      follow-up once the type lands in `src/`.

## 3. Replace the judge with real structural checks

- [x] The **authority axes** run for every standing rule, whatever its action's
      catalog shape, because a standing rule admits its action without
      per-instance approval either way: manifest invariants
      (`StandingRuleManifest::validate`), catalog membership
      (`ActionCatalog::contains`), catalog non-delegability
      (`is_non_delegable`), and composed policy deny.
- [x] A rule whose action carries **no reusable-delegation descriptor** grants
      blanket authority, so its action must be declared approval-narrowing
      (`ActionCatalog::is_approval_narrowing`). Using "has no reviewed scope" as
      the licence instead was the same hole one layer down: `email.send`,
      `secret.rotate`, `filesystem.host_write`, `network.raw_egress`,
      `coolify.deploy` and `policy.modify_direct` all carry no descriptor and
      all passed under it.
  - Tests: `uncatalogued_action_cannot_carry_a_standing_rule`,
    `effectful_actions_cannot_carry_an_unscoped_standing_rule`,
    `approval_narrowing_action_may_carry_an_unscoped_standing_rule`.
- [x] The **scope-bound axes** run where the action carries a
      reusable-delegation descriptor: executor readiness, contract eligibility
      (`validate_delegation_contract`), dark-window admissibility against
      `DarkWindowPolicy`, and budget/expiry bounds from the action's own
      `DelegationPolicyBounds`.
- [x] Overlap and widening against active rules held by a *different* artifact:
      equal `reviewed_scope_digest` is a supersession takeover; an unbound active
      rule covers every scope and so collides with any scoped proposal; and a
      proposal whose bound dimensions are a strict subset of an incumbent's,
      agreeing on every shared one, is a widening.
  - This compares reviewed-scope **dimension maps** directly. The original plan
    said `ReviewedActionScope::compare`, which takes a `ResolvedActionContext`
    and so cannot compare two scopes to each other; the dimension-map comparison
    is the same data by the same equality, without inventing a schema API.
  - Tests: `judge_refuses_policy_denied_action`,
    `judge_refuses_budgets_outside_declared_bounds`,
    `judge_refuses_expiry_outside_declared_bounds`,
    `judge_refuses_scope_already_held_by_another_active_rule`,
    `judge_admits_disjoint_scope_alongside_an_active_rule`,
    `non_delegable_action_cannot_carry_a_standing_rule`,
    `policy_denied_non_scope_bound_rule_is_refused`.
- [ ] Refuse a proposal referencing a quarantined or erased artifact. **Not done,
      and not applicable as written:** a `StandingRuleManifest` references no
      other artifact — it carries an id, an action id, budgets, an optional dark
      window and an optional reviewed scope. There is nothing to check. The
      corresponding clause has been removed from the spec delta rather than left
      as an unenforced requirement.

## 4. Make replay execute cases

- [x] `overlay_eval_gate/replay_cases.rs` derives the case set from the
      proposal's own bound reviewed scope: a baseline case reconstructing the
      resolved context that scope describes, plus one changed-context case per
      bound **instance** dimension.
- [x] Every case runs through the production predicate — a real
      `ResolvedActionContext::try_new` followed by `ReviewedActionScope::compare`
      — never a private copy.
- [x] The executed-case ledger records kind, mutated dimension, expected and
      observed outcome, and whether the observed mismatch was **attributed** to
      the dimension the case varied.
- [x] Deny unless the ledger is non-empty, contains a matching case, contains a
      refused changed-context case, and every observed outcome equals its
      expected outcome.
- [x] No `fitness` for this evaluator: required case classes are pass/fail.
- [x] Declaration axes are listed explicitly rather than caught by `_`, so a
      newly added instance dimension fails to compile instead of silently going
      uncovered. `AccountRole` and `RelationshipTier` were swallowed by a
      catch-all until review caught it; both are now varied.
  - Tests: `replay_executes_matching_and_changed_context_cases`,
    `mutated_dimension_cases_are_refused_and_name_the_dimension` (asserts all ten
    bound instance dimensions are exercised).
- [ ] Positive cases reconstructed from the sealed evidence set. **Not done:**
      blocked on the same missing `DelegationEvidence` production type as task 2.
      The baseline case plays the role the evidence positives were meant to —
      it is the scope the owner reviewed — but it is derived from the binding,
      not from approval events.
- [ ] Budget-exhausted, policy-deny, missing-executor and ambiguous-overlap
      **replay cases**. **Not done as replay cases, and deliberately so:** each is
      a pure property of the proposal, not of a varied context, so each is a
      structural judge axis (task 3) that refuses the proposal outright. Running
      them again as simulated cases would report a second opinion on a question
      already answered. Recorded here rather than silently dropped.
- [ ] Dark-window adversarial cases as replay cases. **Not done as replay cases:**
      dark-window admissibility is a judge axis over
      `DarkWindowPolicy::{Prohibited, DenyOnly, BoundedAllow}`. `bound-dark-window-exceptions`
      (#135) has no artifacts or symbols in any tree, so the adversarial case set
      it would supply cannot be written yet.

## 5. Bind epochs and stale at read time

- [x] The stored verdict carries `proposal_digest`, `compatibility_digest`,
      `reviewed_scope_digest`, `evidence_set_digest`, and descriptor,
      implementation and policy versions, within the D-056 open-vocabulary schema
      (migration v6, nullable columns, no backfill).
- [x] Currency is computed at read time by comparing stored epochs against live
      values. No sweeper, no mutating pass.
- [x] Activation re-checks currency before its transaction opens, so the refusal
      audit survives.
  - Tests: the nine `eval_verdict_epoch_tests`, plus
    `stale_verdict_cannot_support_activation` and
    `activation_path_refuses_a_stale_verdict`.
- [x] Evaluator identity moved to `@v2`, so a stored `@v1` verdict is
      distinguishable from a post-change one. The D-062 startup provenance check
      matches on the `overlay-eval-gate/replay@` prefix and still passes.
- [ ] Re-check currency at **promotion**. **Not done, and vacuous as specified:**
      promotion computes the epochs it stores in the same operation, so it cannot
      observe a stale verdict. The spec delta now says activation only, and
      records that the axis promotion enforces is the digest binding both
      witnesses already carry.

## 6. Render owner copy from the ledger

- [x] `overlay_eval_gate/summary.rs` renders from the two verdicts' recorded
      evidence. No free-text field survives.
- [x] Each evaluator is named from its own evidence, so the word *replay* cannot
      appear beside a pass on a proposal where nothing ran.
  - Tests: `summary_reports_executed_case_counts_and_claims_no_more`,
    `availability_only_evaluation_makes_no_replay_claim`.

## 7. Prove the boundary end to end on the real path

- [x] `propose_path_refuses_a_non_delegable_standing_rule_before_review_required`
      drives `dispatch_artifact_propose`, so the `AssemblySources` construction
      and the `eval.epochs` plumb-through inside it are exercised, and asserts
      the proposal never reaches `review_required`.
- [x] `activation_path_refuses_a_stale_verdict` drives
      `commit_artifact_activation`, so the currency re-check is proven wired into
      activation rather than merely callable on the store.
- [x] `passing_evaluation_grants_no_authority` asserts no active rule, no
      reservation and no pending-write fence after a pass.

## 8. Document and record (authority-sensitive)

- [x] Decision-log entries **D-163**, **D-164**, **D-165** (D-158 was highest in
      both this tree and the peer tree at the moment of writing), with a dated
      Change Log row.
- [x] The MODIFIED requirements in `artifact-lifecycle` and
      `lineage-and-eval-store` carry current post-#128 text; nothing is
      re-`ADDED`; the pre-seeded "Proposal without captured owner history is
      denied" scenario is retained rather than dropped, because
      `ReplayDenial::NoOwnerHistory` still enforces it.
- [x] Authority boundary verified: a passing evaluation creates no grant, no
      active rule, no reservation and no pending row.
- [x] AD-142 promotion boundary non-regression: the witness types keep private
      fields and `promote_authority_bearing_proposal` remains the only
      `validated -> review_required` path.
- [x] Model-swap golden-set path non-regression: unchanged apart from sharing the
      verdict epoch binding.
