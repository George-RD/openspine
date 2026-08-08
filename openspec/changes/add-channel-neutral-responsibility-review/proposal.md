# Add channel-neutral responsibility review and lifecycle controls

## Dependencies

The per-brief `Requires:` line names exactly one **HARD** prerequisite:

- `define-responsibility-contract` (archived, **HARD**): supplies `OwnerReviewRequest`, `ReviewedActionScope`, `DelegationEvidence`, `ProposalProvenance`, `OwnerReviewDecision`, `ResponsibilityLifecycleControl`, and the D-146 two-axis descriptor/implementation catalog this change consumes. The semantic review object and its binding digest exist and are tested, but they have **zero kernel persistence, zero decision handling, and zero owner-surface rendering** today.

Archived-context (non-blocking) dependencies:

- `unify-approved-and-delegated-effect-execution` (#127, archived): registered the `gmail.create_draft` executor and established that a proposal's claim that the reusable effect path is ready must be backed by a registered executor (`AppState::is_execution_backed`). Proposal copy rendered by this change MUST NOT outrun that executor readiness.

Boundary-only (explicitly **non-blocking**, in-flight in parallel worktrees, MUST NOT be taken as a prerequisite):

- `mine-and-match-reusable-authority-by-scope` (#128, in-flight): binds standing rules to reviewed scopes and defines exact-one-match admission. This change consumes the scope-bound standing rule as the thing a responsibility's lifecycle controls (pause/resume/revoke) act upon, and it consumes `ReviewedActionScope` from the **archived** `define-responsibility-contract` contract (already in the tree at `crates/openspine-schemas/src/reviewed_scope.rs`) so a narrowed review object still carries an enforceable, digest-bound reviewed scope. #128 is in-flight right now and its matching/budget semantics are **not** prerequisites for this change; the seam we must not cross is drawn in design.md and tasks.md (we own the owner-facing review object and lifecycle intents; #128 owns scoped matching). If #128's additions land first, we consume them; we never block on them, and we never re-author matching or budget semantics.

Canonical decisions: D-146, D-011 (WYSIWYS digest-bound approval), D-107 (standing rules as reviewed composition inputs), plus AD-036, AD-120, AD-148.

This change affects **OpenSpine core** and the two owner surfaces (Telegram and the local terminal). It introduces no connector access, no external communication, and no system-operations authority. It makes owner-facing review and lifecycle decisions portable across surfaces while keeping every decision kernel-verified and digest-bound.

## Problem/Context

D-146 defines `OwnerReviewRequest` as the single semantic review object that Telegram, terminal, and future owner surfaces must render and submit against. That contract is real and tested in `crates/openspine-schemas/src/owner_review.rs`, but it is inert: nothing persists it, nothing decides it, and no surface renders it.

Today owner-facing control is Telegram-shaped and decision-poor:

- Approval is Telegram-only and Approve-only. The pipeline at `crates/openspine-kernel/src/pipeline/mod.rs:423-453` routes callback data through a per-feature parser chain — `parse_standing_rule_callback`, `parse_approve_callback`, `parse_approve_plan_callback` — each carrying its own `bound_chat_id: i64` and its own handler. The local terminal lane (`pipeline/lanes.rs:137 terminal_owner_lane()`) authenticates the owner via `Source::Cli` + `VerificationMethod::LocalCliAuth`, but there is **no terminal decision surface at all**: a terminal owner cannot approve, reject, narrow, pause, resume, or revoke anything.
- The storage/API seam is Telegram-shaped even outside the renderer. `bound_chat_id: i64` threads through `api/actions.rs` (`mediate_and_dispatch_action`), `pipeline/approval.rs:127` (channel-mismatch check), `pipeline/plan_approval.rs:83`, and `store/standing_rules_pending.rs` (pending rows carry `bound_chat_id`). Generic review/decision/receipt code has no channel-neutral reply target.
- There is no `paused` standing-rule state. The store writes free-form `status TEXT` (`active`, `needs_review`, `revoked`) at `store/standing_rules.rs:105-110` with no `CHECK` constraint, and every live-consultation site filters strictly `status = 'active'` (`active_standing_rule_for_action`, `standing_rules_budget.rs:49,115,315`, `standing_rules_fired_token.rs:89-92`). Pause therefore does not exist, and there is no compatibility revalidation on resume.

The consequence is that the review lifecycle is not portable: the object the owner is shown and the decision the owner makes cannot be guaranteed identical across surfaces, and the lifecycle controls the brief requires (Reject, Narrow, Pause, Resume, Revoke, inspect, receipt) do not exist anywhere.

## Proposed Solution

1. **Persist the canonical review object as one content-addressed, digest-bound record.** Serialize the `OwnerReviewRequest` plaintext and store it through the existing `ArtifactStore::put` (`crates/openspine-kernel/src/artifact_store_io.rs:8`), keeping the returned `ArtifactRef` and the object's own `binding_digest` as the stored reference. Retrieval reuses `ArtifactStore::get` (`artifact_store_io.rs:73`), which re-verifies the digest at read time, and re-validates `OwnerReviewRequest::binding_is_valid()`. Add a `review` store row keyed by review id, its `ArtifactRef`, its lifecycle state, the bound owner principal id, and `expires_at`, with an `append_audit_conn` write inside the same transaction. Refuse to persist an approvable review whose full owner-facing rendering would be truncated on the target channel, so oversized content cannot leave a persisted unreviewable proposal (this mirrors the existing truncation rule at `api/actions.rs` `dispatch_lyra_preview` and the `digest-bound-draft-approval` spec).

2. **Add a typed review lifecycle and principal-bound decision intents.** Introduce an `OwnerReviewState` (Pending / Approved / Rejected / Narrowed / Revoked / Expired) with a `can_transition` validator, mirroring the `Lifecycle` enum-plus-validator idiom (`crates/openspine-schemas/src/artifact.rs:60`). Wire principal-bound `DecisionIntent` values — Approve, Reject, Narrow, Edit, Pause, Resume, Expire, Revoke, Inspect — through the existing kernel-verified decision path so every decision is (a) authenticated to the owner principal, (b) digest-bound to the stored review object, and (c) audit-recorded. **Approve and Narrow reuse the digest-bound approval guarantee (D-011 WYSIWYS)**: the decision is bound to the same binding digest the owner was shown and re-verified at effect time; the review row is the sole disposition of the decision, and the approved effect still crosses `gate()` under a fresh task grant (D-007).

3. **Add a renderer abstraction with Telegram and terminal implementations.** A kernel-owned owner-surface adapter renders the one semantic `OwnerReviewRequest` and submits a principal-bound decision intent back against the same binding digest. Telegram is verified via `VerifiedOwnerContext`; terminal is verified via the `local_cli_owner` envelope (`Source::Cli` + `LocalCliAuth`). Neither adapter may add scope, decisions, or lifecycle logic absent from the stored object — they are authenticated presentation/input adapters only.

4. **Add paused state and compatibility-revalidated resume.** Add `paused` to the standing-rule status set with a `CHECK(status IN (...))` constraint, and audit every live-consultation filter so paused rules are absent from live consultation (a pause immediately restores ordinary approval). Add `pause_standing_rule` and `resume_standing_rule` alongside `revoke_standing_rule` (`store/standing_rules.rs:277`), each writing a distinct audit event. Model resume on the `artifact.reconfirm` ceremony (`pipeline/artifact_reconfirmation.rs`): re-verify the reviewed bytes and digest, re-check the rule is still the exact paused version, and revalidate policy, descriptor, executor readiness, connector/account, and reviewed scope before returning to `active`; refuse to resume an expired or drifted rule and require a new reviewed version instead. Use the `committed: bool` "race already handled, say nothing" idiom so a concurrent resume tap is a safe no-op.

5. **Keep proposal copy truthful.** Owner-facing copy must not outrun evidence (the `DelegationEvidence`/`ProposalProvenance` provenance), enforceable scope (the `ReviewedActionScope` digest), or executor readiness (the `is_execution_backed` two-part catalog query from #127). Review receipts are post-decision and truthful to what the decision committed: they name the review, the intent and the binding digest, and never claim an external effect executed. The effect's own `EffectOutcome` receipt contract from #127 stays on the effect path, unchanged.

## Acceptance Criteria

- Telegram and terminal decide the **same stored review object**: rendering either surface against one persisted `OwnerReviewRequest` yields the same binding digest, and a decision submitted through either surface is recorded against the same review row and the same digest.
- A narrowed review creates a **new immutable digest**: Narrow produces a new `OwnerReviewRequest` whose binding digest differs from the original, is persisted as its own content-addressed record, and cannot be confused with or substituted for the original.
- **Pause / resume / revoke are replay-safe**: a duplicate or concurrent Pause, Resume, or Revoke intent is a safe no-op (or an idempotent already-handled outcome), leaves exactly one durable audit disposition, and never double-transitions the standing-rule status (`active`/`paused`/`revoked`).
- A paused rule is absent from live consultation: while paused, the action requires ordinary owner approval and no scoped rule may admit it.
- Resume reactivates only a still-compatible, still-current reviewed version: byte/digest re-verification, version-staleness re-check, policy/descriptor/executor/connector/account/scope revalidation all pass before `active`; an expired or drifted rule is refused and requires a new reviewed version, with a distinct audit event per rejection reason.
- Proposal copy cannot outrun evidence, enforceable scope, or executor readiness: rendered copy is derived from stored provenance, names only the reviewed scope digest, and never claims the reusable effect path is ready unless `is_execution_backed` is true.
- An approvable review whose full rendering would be truncated on the target channel is never persisted as approvable.
- `node_modules/.bin/openspec validate add-channel-neutral-responsibility-review --strict` passes, and every delta requirement whose header already exists in its pre-seeded capability spec is carried as `## MODIFIED Requirements`.

## Invariant

**Owner channels are authenticated presentation/input adapters, never authority or lifecycle logic.** No Telegram callback, no terminal command, and no future surface may add scope, mint a decision, change a lifecycle state, or re-derive authority. Every decision is principal-bound, digest-bound to the stored review object, kernel-verified, and audit-recorded; every lifecycle transition is kernel-owned. The renderer abstraction may only present the stored object and submit an authenticated intent.

## Out of Scope

- Re-opening boundaries #127 settled: the shared Gmail executor, the `EffectOutcome` truthfulness contract, and the closed non-effect stub allowlist are unchanged.
- Re-opening boundaries that are #128's, not this change's: scoped matching, the `reviewed_scope_digest` vs `compatibility_digest` split, exact-one-match admission, and the `EffectOutcome` → reservation mapping. This change **consumes** the scope-bound standing rule and `ReviewedActionScope` (from the archived contract) for narrowing and lifecycle; it does not re-author matching or budget semantics, and it does not duplicate or contradict #128's behavior.
- The evidence-class mining and the repeated-approval → proposed-responsibility grouping remain in `define-responsibility-contract` and are not re-authored here.
- The scoped outstanding-pending-exception cap and dark-window Allow posture remain `bound-dark-window-exceptions` (#135) and are unchanged.
- No second connector, resolver, or executor is introduced. No widening of `email.create_draft` authority and no authorization of `email.send`.
- No new OAuth, credential, or principal model: this change binds the existing owner `Principal` to decisions and does not add a web/mobile surface (that is a future change).
