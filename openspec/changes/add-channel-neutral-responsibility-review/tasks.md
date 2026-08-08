# Tasks

## 1. Rebase onto main after #128 archives, then re-derive deltas

- [x] Do NOT begin implementation until `mine-and-match-reusable-authority-by-scope` (#128) is archived on `origin/main`, because #128 and this change both edit `crates/openspine-kernel/src/pipeline/mod.rs` and `crates/openspine-kernel/src/store/standing_rules.rs`.
- [x] `git fetch origin` and rebase `change/add-channel-neutral-responsibility-review` onto fresh `origin/main`.
- [x] Rebase again onto `origin/main` after `bound-dark-window-exceptions` (#135) archived: branch fast-forwarded to `e7541ab`, all ten conflicts resolved, migrations renumbered v7/v8 behind #135's v6.
- [x] Re-derive every `## MODIFIED Requirements` body against the updated `openspec/specs/<capability>/spec.md` (post-#128-archive), especially `responsibility-contract` and `standing-rules`, so a MODIFIED block never restates text #128 already landed.
- [x] Re-run `node_modules/.bin/openspec validate add-channel-neutral-responsibility-review --strict` and `node_modules/.bin/openspec validate --all --strict`.

## 2. Persist the canonical review object, content-addressed and digest-bound

- [x] Serialize the `OwnerReviewRequest` plaintext and store it through `ArtifactStore::put`, keeping the returned `ArtifactRef` as the stored reference; retrieval reuses `ArtifactStore::get` with its read-time digest re-verification and re-checks `OwnerReviewRequest::binding_is_valid()` before any decision is eligible.
- [x] Add a `review` store row keyed by review id, its `ArtifactRef`, its `OwnerReviewState`, the bound owner principal id, and `expires_at`, with an `append_audit_conn` write inside the same transaction as every state transition.
- [x] Refuse to persist an approvable review whose full owner-facing rendering would be truncated on the target channel, so oversized content cannot leave a persisted unreviewable proposal. Reuses D-045's `truncate_for_telegram` as the fit test (`OwnerReviewRenderer::fits`) rather than a second size budget; test `oversized_review_is_not_persisted_as_approvable`.

## 3. Add a typed review lifecycle and principal-bound decision intents

- [x] Add an `OwnerReviewState` enum (Pending / Approved / Rejected / Narrowed / Revoked / Expired) with a `can_transition` validator, mirroring the `Lifecycle` enum-plus-validator idiom (`crates/openspine-schemas/src/artifact.rs:60`).
- [x] Add principal-bound `DecisionIntent` values (Approve, Reject, Narrow, Edit, Pause, Resume, Expire, Revoke, Inspect) wired through the existing kernel-verified decision path so every decision is authenticated to the owner principal, digest-bound to the stored review object, and audit-recorded. One enumerated refusal has no named test: `ExecutorUnavailable` (Approve for an action with no registered executor). It is enforced at `owner_review_decision.rs:228` and enumerated in the spec's typed-refusal requirement, but every review fixture is bound to `email.create_draft`, which IS execution-backed, so reaching the arm needs a second scoped action in the catalog. Recorded rather than scenario-tested.
- [x] Approve and Narrow reuse the digest-bound approval guarantee (D-011 WYSIWYS): the decision binds the same binding digest the owner was shown and re-verifies it at effect time; the review row is the sole disposition of the decision, and the approved effect still crosses `gate()` under a fresh task grant (D-007).

## 4. Narrow produces a new immutable digest

- [x] Implement Narrow as a new-review-producing decision: the kernel constructs a new `OwnerReviewRequest` with a narrowed `ReviewedActionScope` and a new binding digest, persisted as its own content-addressed record.
- [x] Preserve per-dimension comparability so narrowing one dimension does not force re-review of the rest, and ensure the narrowed scope remains a valid, matchable `ReviewedActionScope` for scoped admission.
- [x] Prove the original review object stays immutable and a narrowed decision cannot be replayed as approval of the broader original.

## 5. Add an owner surface reference contract

- [x] Introduce a kernel-owned `OwnerSurfaceRef`/reply-target contract that represents verified Telegram private owner chat, authenticated local terminal/device session, future web/mobile owner session, and optional channel-thread binding (AD-148).
- [ ] Make the surface reference typed, principal-bound, and authenticated/MAC-covered where it crosses a grant; keep Telegram ids inside adapter storage and separate from connector-specific rendering ids. **Partially done:** the ref is typed and principal-bound and Telegram ids are adapter-only, but `task_grants.owner_surface_json` is a sibling column and is NOT inside the grant's MAC envelope (`TaskGrant::verify_mac` covers `grant_json` only), so a direct DB write could repoint a grant's reply surface. Closing this means either moving the surface into the sealed grant or MAC-ing the column.
- [x] Ensure generic review, decision, pending-action, notification, and receipt code accepts no naked `bound_chat_id: i64`.
- [x] Prove terminal review never fakes a Telegram chat id to reuse the lifecycle, and existing Telegram grants/pending rows remain valid or fail closed (migration/compat tests).

## 6. Add a renderer abstraction with Telegram and terminal implementations

- [x] Add a kernel-owned renderer trait with Telegram and terminal implementations that present the one stored `OwnerReviewRequest` and submit a principal-bound `DecisionIntent` against the same binding digest.
- [x] Verify Telegram through `VerifiedOwnerContext` and the terminal through the `local_cli_owner` envelope (`Source::Cli` + `LocalCliAuth`).
- [x] Prove neither adapter can add scope, decisions, or lifecycle logic absent from the stored object (presentation/input adapters only).

## 7. Add paused state and revalidated resume

- [x] Add `paused` to the standing-rule status set and add a `CHECK(status IN (...))` constraint to `store/standing_rules.rs`.
- [x] Audit every live-consultation site that filters `status = 'active'` (`active_standing_rule_for_action`, `standing_rule_is_current`, `reserve_standing_rule_budget`, the currency re-check in `finalize_standing_rule_reservation`, and the `EXISTS` guard in `standing_rules_fired_token.rs:89-92`) so a paused rule is absent from live consultation.
- [x] Add `pause_standing_rule` and `resume_standing_rule` alongside `revoke_standing_rule`, each writing a distinct audit event (`standing_rule.paused`, `standing_rule.resumed`); keep revoke immediate and idempotent.
- [x] Decide and document supersession: a new version activating over a paused rule transitions it to `revoked` so a stale paused rule cannot silently reappear on a later resume.
- [x] Model resume on the `artifact.reconfirm` ceremony: re-verify bytes/digest, re-check the rule is still the exact paused version, and revalidate policy, descriptor, executor readiness, connector/account (`ConnectorRegistry::breaker_state`, no connector-specific branches), and reviewed scope before returning to `active`.
- [x] Refuse to resume an expired or drifted rule and require a new reviewed version; write a distinct audit event per rejection reason.
- [ ] Use the `committed: bool` "race already handled, say nothing" idiom so a concurrent resume tap is a safe no-op. **Partially done:** the conditional `UPDATE ... WHERE status = 'paused'` guarded by `changes() == 1` makes a duplicate tap a no-op. Genuinely concurrent taps against the *store* transition are covered by `standing_rules_lifecycle_tests::concurrent_lifecycle_intents_write_one_transition_audit`, and the sequential replay of the *revalidated* path by `resume_of_an_already_active_exact_version_is_a_safe_noop`. The remaining gap is narrow and specific: no test drives two concurrent calls through `resume_standing_rule_revalidated`, so the claim that the revalidation-then-flip sequence is race-free rests on `BEGIN IMMEDIATE` by inspection.

## 8. Keep proposal copy and receipts truthful

- [x] Derive owner-facing copy from stored `ProposalProvenance`; never claim an observed pattern unless `DelegationEvidenceKind::RepeatedApprovals` supports it; name only the reviewed scope digest.
- [x] Never claim the reusable effect path is ready unless `AppState::is_execution_backed` is true (executor readiness from #127).
- [ ] Emit post-use receipts truthful to the actual effect outcome (`EffectOutcome` from #127); never restate a false success. **Not done:** review receipts today describe only the committed lifecycle/authority outcome and deliberately never claim an external effect ran, which is honest but is not the same as reporting the effect's `EffectOutcome`. No named test asserts a receipt against an actual effect result.

## 9. Test the boundaries

- [x] Telegram and terminal decide the same stored review object: rendering either surface against one persisted review yields the same binding digest, and a decision through either surface records against the same review row and digest.
- [x] Narrow creates a new immutable digest and cannot be replayed as the broader original.
- [x] Pause / resume / revoke are replay-safe: duplicate or concurrent intents are safe no-ops, leave one durable audit disposition, and never double-transition.
- [x] A paused rule is absent from live consultation and ordinary approval is restored immediately.
- [ ] Resume reactivates only a still-compatible, still-current reviewed version; expired/drifted rules are refused with a distinct reason. **Partially done:** expired, superseded, missing-scope, connector-unavailable and the happy path each have a named test (`resume_refuses_*`, `resume_reactivates_a_still_current_rule`). Drift is now covered by `resume_refuses_a_rule_whose_compatibility_epoch_drifted`. One arm remains implemented-but-untested: a corrupt persisted binding reaching `resume_refused_invalid_scope` via `binding_is_valid()` (the existing `resume_refuses_a_rule_with_no_reviewed_scope` reaches that audit kind by the missing-scope path, not the corrupt-binding path).
- [x] Proposal copy cannot outrun evidence, enforceable scope, or executor readiness.
- [x] An approvable review whose rendering would be truncated on the channel is never persisted as approvable.
- [x] Migration/compat: existing Telegram grants and pending rows remain valid or fail closed; terminal review never fakes a Telegram id.

## 10. Document and record (authority-sensitive)

- [x] Record the decisions (persist-whole review object, dedicated `OwnerReviewState`, `OwnerSurfaceRef`, Narrow-produces-new-digest, paused-as-typed-status, reconfirm-shaped resume) as a new decision-log entry (index row, full section, Change Log row).
- [x] Update the capability specs; confirm every requirement already present in a pre-seeded spec is carried as `MODIFIED`.
- [x] Verify the invariant end to end via the falsifiable scenarios: a surface cannot add scope or mint a decision outside the stored object (`responsibility-contract` "A surface submits a decision outside the stored object"), cannot mutate lifecycle state (`direct-terminal-chat` "Terminal cannot mutate lifecycle state"), and cannot re-derive authority (`direct-terminal-chat` "Terminal cannot re-derive authority").
- [x] Confirm no #127 boundary regressed: `NoExecutor` stays an opaque `500 {"error": "internal_error"}`, the non-effect stub allowlist stays closed, and the shared executor and `EffectOutcome` truthfulness contract are unchanged.
- [x] Confirm no #128 boundary is duplicated or contradicted: matching, `reviewed_scope_digest` vs `compatibility_digest`, exact-one-match admission, and the `EffectOutcome` → reservation mapping remain #128's.
