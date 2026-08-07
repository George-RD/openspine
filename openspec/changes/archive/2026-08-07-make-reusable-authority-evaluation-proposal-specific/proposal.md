# Make reusable-authority evaluation proposal-specific and evidence-backed

## Dependencies

- `define-responsibility-contract` — **HARD**, archived. Supplies `ResolvedActionContext`,
  `ReviewedActionScope`, `DelegationEvidence`, `ProposalProvenance`, and the
  two-axis catalog (`ActionDescriptor` + `ActionImplementationDescriptor`).
- `mine-and-match-reusable-authority-by-scope` (#128) — **HARD**, archived. Supplies the
  `reviewed_scope_digest` / `compatibility_digest` split (D-155), exact-one-match
  admission (D-156), the `EffectOutcome` → reservation map (D-157), and
  activation-time scope-binding enforcement.
- Canon: **D-146** (responsibility is a reference view; sealed generic scope; distinct
  evidence classes; communication dark-window `Allow` forbidden), **D-060** (the overlay
  eval gate's first-cut evaluator is deterministic and the full protocol is
  owner-reserved), **D-107** (standing rules are composition inputs with fail-closed
  reservation semantics). This change is the owner-ratified evaluator upgrade that
  D-060's "Would change if" anticipated.
- Layer: OpenSpine core (kernel evaluation boundary), not Lyra package content.
- Authority-sensitive: **yes** — it governs what may reach an owner approval tap.

## Problem/Context

The AD-142 promotion boundary is structurally sound and must be kept. `ReplayPassed`
and `JudgePassed` (`crates/openspine-kernel/src/overlay_eval_gate/mod.rs:89` and `:114`)
have private fields and no public constructor;
`Store::promote_authority_bearing_proposal` is the only operation that can perform the
`validated -> review_required` transition, it consumes both tokens by value, and it
re-derives the digest from the stored row before promoting. A caller cannot fabricate a
token or reuse one across proposals.

What that unforgeable boundary currently gates on is the problem.

**The "replay" evaluator does not replay anything.** For every non-`model_swap`
proposal, `overlay_eval_gate/replay.rs:76-79` is:

```rust
let owner_turns = store.count_owner_control_conversation_turns()?;
if owner_turns == 0 {
    return Err(ReplayDenial::NoOwnerHistory);
}
```

It then returns `verdict: "pass"` with `fitness: Some(1.0)` and evidence
`{"corpus": "owner-control-conversation", "captured_turns": N}`. The proposal's own
content is never applied to anything. A perfect fitness score is recorded for the fact
that the owner has ever spoken.

**The "risk judge" checks catalog membership.** For a standing rule,
`overlay_eval_gate/judge.rs:98-101` pushes exactly one value —
`declared.push(&rule.action_id)` — and the only assertions are
`catalog.contains(action)` and absence of an allow/deny conflict
(`judge.rs:120-133`). Executor readiness, reviewed-scope completeness, policy deny,
budget bounds, overlap with an active rule, and evidence integrity are all unchecked.

**The owner is then told this was a replay.** `run_gate` composes
(`mod.rs:160-166`):

```rust
"AD-142 overlay eval gate — replay: {} ({}); risk judge: {} ({})"
```

so the approval surface carries the word *replay* over evidence that proves only corpus
presence. Issue #133 records that PR #125 shipped exactly this copy failure.

**Nothing goes stale.** `eval_verdicts` rows bind `artifact_digest` only
(`crates/openspine-kernel/src/store/eval_verdict_store.rs`). A verdict computed against
one descriptor version, executor registration, policy set, or reviewed scope stays
valid-looking after any of them change.

**The corpus the brief assumes does not exist.** There is no table of prior
`ResolvedActionContext` rows. Contexts are built ephemerally at scoped admission
(`crates/openspine-kernel/src/api/scoped_admission.rs`) and discarded. `approvals`,
`action_requests`, and `conversation_state` hold digests and intents, not sealed
resolved contexts. Any design phrased as "replay the proposal against stored prior
contexts" would therefore have to degrade into a presence check with a confident
label — which is the defect this change exists to remove.

## Proposed Solution

1. **Assemble a kernel-derived, digest-bound evaluation input, or refuse.**
   A new `CanonicalEvaluationInput` is built only by the kernel from the parsed
   proposal, `canonical_catalog()`, executor readiness, the composed deny set, and the
   active artifact registry. It carries the action descriptor and implementation
   descriptor, executor/resolver readiness, the proposal's `ReviewedScopeBinding`, the
   evidence-set digest, budgets and expiry, and the epoch set defined in move 4. Any
   dimension that cannot be resolved yields a typed `IncompleteInput` naming the missing
   dimension — never a pass. This preserves D-146's fail-closed construction rule.

2. **Replace the judge body with deterministic structural checks over that input.**
   Each check reuses an existing primitive rather than inventing a second opinion:
   `validated_delegation_contract` and `reusable_delegation` for delegability;
   `AppState::is_execution_backed` for executor and resolver readiness;
   `required_scope_dimensions_for` plus `ReviewedScopeBinding::binding_is_valid` for
   scope completeness and integrity; the composed `denied_actions` set for policy deny;
   `ActionDescriptor::is_communication_or_connector_write` and `DarkWindowPolicy` for
   effect-class and dark-window admissibility; `BudgetWindowBounds::contains` and
   `maximum_lapse_secs` from the action's own `DelegationPolicyBounds` for limits;
   `ReviewedActionScope::compare` plus the #128 dual-digest semantics for overlap and
   widening against active rules; and `Lifecycle::Quarantined` / `CompatibilityStatus::Erased`
   for referenced-artifact health. Every failure is a typed denial naming the axis.

3. **Make replay mean executed cases, and make a case-free pass impossible.**
   Replay runs the exact proposed binding against a case set derived from the proposal's
   own bound inputs: **positive cases** reconstructed from the evidence set the proposal
   carries (`DelegationEvidence::repeated_approvals` seals a `context_class_digest` and
   its approvals), and **changed-context cases** generated by mutating each bound scope
   dimension in turn, plus budget-exhaustion, policy-deny, missing-executor, and
   ambiguous-overlap variants. Each case is executed against the real matching predicate
   and its outcome recorded. The verdict carries the executed-case ledger, and the
   evaluator **denies** unless the ledger is non-empty and contains at least one case
   that matched and at least one changed-context case that did not. A corpus-presence
   check cannot satisfy that shape, so the word cannot drift back onto it.

4. **Bind verdicts to an epoch set and stale them when it moves.**
   Each stored verdict carries `proposal_digest`, `compatibility_digest`,
   `reviewed_scope_digest`, `evidence_set_digest`, and the descriptor, implementation,
   and policy versions it was computed under. A verdict is current only while every one
   of those still equals the live value; otherwise it is stale and cannot support
   promotion or activation. Staleness is evaluated at read time from stored columns, so
   no sweeper is required and a crash cannot leave a stale verdict looking fresh.

5. **Derive owner copy from the ledger.**
   The gate summary is generated from the executed-case ledger and the structural axes
   that passed — never free text. It states the number and kind of cases actually run.
   With no ledger there is nothing to render, so the summary cannot claim a replay that
   did not happen.

## Acceptance Criteria

- A standing-rule proposal whose action has no registered executor cannot reach
  `review_required`: the gate denies with an executor-readiness reason, no verdict rows
  claim a pass, and no owner approval tap is sent.
- A proposal whose reviewed-scope binding omits a required dimension, or whose stored
  digest disagrees with its stored values, is denied as incomplete input naming the
  missing or inconsistent dimension, rather than evaluated as a generic artifact.
- A proposal for an action the composed policy denies is denied by the structural judge
  even when every other axis passes.
- A proposal whose budgets or expiry fall outside the action's own
  `DelegationPolicyBounds` is denied, and one inside them passes that axis.
- A proposal whose reviewed scope equals an active rule's scope, or widens it, is denied
  as an ambiguous or widening overlap; a disjoint scope is not.
- Replay executes concrete cases: a stored passing verdict names at least one matching
  case and at least one changed-context case that did not match, each with its outcome,
  and an evaluation that executed zero cases is a denial rather than a pass.
- A changed-context replay case built by mutating one bound scope dimension does not
  match the proposed binding, and the ledger records which dimension was mutated.
- A verdict is stale when any bound epoch changes: mutating the compatibility digest,
  the reviewed scope digest, the evidence-set digest, or the descriptor, implementation,
  or policy version makes the stored verdict non-current, and promotion or activation
  reached with only a stale verdict is refused.
- The owner-facing gate summary states only what the ledger proves: it reports the
  executed case counts and passing axes, and contains no replay claim when no cases ran.
- Evaluation grants nothing: a passing evaluation leaves the proposal in
  `review_required` with no active rule, no task grant, no budget reservation, and no
  dark-window pending row.
- `node_modules/.bin/openspec validate make-reusable-authority-evaluation-proposal-specific --strict`
  passes, and every delta requirement whose header already exists in its pre-seeded
  capability spec is carried as `## MODIFIED Requirements`.

## Invariant

**Evaluation neither grants nor activates authority, and owner-facing copy may state
only what the stored exact-proposal verdicts prove.** A passing evaluation moves a
proposal to `review_required` and nothing else — it mints no grant, activates no rule,
reserves no budget, and schedules no timer. A check may be called *replay* only when
concrete cases were executed against the exact proposed binding and their outcomes
stored; a corpus-presence or availability check MUST be named for what it measures. Any
verdict whose bound epochs no longer match the live values is stale and supports
nothing.

## Out of Scope

- **The full OQ-17 holdout-replay and AD-111 prover-verifier protocol.** D-060 reserves
  evaluator independence and attack-trace formalism for owner ratification. This change
  upgrades the evaluator content under the existing AD-142 promotion boundary and the
  D-056 verdict schema; it does not settle judge independence.
- **A durable corpus of prior resolved action contexts.** None exists, and this change
  does not create one. Replay cases are derived from the proposal's own bound evidence
  set and from generated mutations of its own reviewed scope. Persisting real historical
  contexts for holdout replay is follow-up work.
- **`bound-dark-window-exceptions` (#135).** That change is being implemented in
  parallel and has **no OpenSpec artifacts and no code symbols in any tree** at the time
  of writing — only a sequence brief and a GitHub issue. This change therefore consumes
  only the dark-window substrate already on `main`: `DarkWindowPolicy::{Prohibited,
  DenyOnly, BoundedAllow}`, `DarkWindowDefault`, and the
  `CommunicationDarkWindowAllowForbidden` rule in `validate_delegation_contract`. Its
  dark-window adversarial cases assert those existing rules. When #135 lands its
  exception-budget vocabulary, the case set extends to cover it; this change deliberately
  does not guess at those names.
- **Owner review-object copy.** `add-channel-neutral-responsibility-review` (#129) owns
  the review object and is adding its own requirement about proposal copy in
  `responsibility-contract`. This change governs only the propose-time gate summary in
  `artifact-lifecycle` and does not touch that requirement.
- **A global product-maxima configuration.** No cross-action maxima table exists; limits
  are validated against the proposed action's own `DelegationPolicyBounds`. Introducing
  a product-wide envelope is separate work.
- **Model-swap evaluation.** The golden-set path (`run_model_swap_gate`) already executes
  concrete cases and is left as-is apart from sharing the new verdict epoch binding.
