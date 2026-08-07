# Tasks

## 1. Add the reviewed-scope key to the resolved context

- [x] Add `ResolvedActionContext::reviewed_scope_digest()` computed over exactly the values named by `required_scope_dimensions`, sealed with `digest_of(&serde_json::json!({…}))` in the dimension order used by `validate_required_dimensions`.
  - Sealed via `digest_of` over a JSON object keyed by the snake_case dimension names (`reviewed_scope::reviewed_scope_digest_of`). Insertion order is immaterial because `canonical_json` sorts keys at every depth, so two contexts agreeing on every required dimension always produce byte-identical pre-images.
- [x] Add a per-dimension reviewed-value accessor so a caller can persist and compare individual dimensions, not only the digest.
- [x] Keep `compatibility_digest()` byte-identical: it remains the drift epoch and MUST NOT gain an instance axis.
  - `git diff crates/openspine-schemas/src/resolved_context.rs` touches only additions after `compatibility_digest()`; the `digest_of` pre-image at lines 144-159 is unchanged.
- [x] Prove the two digests are independent — a context differing only in an instance axis keeps its compatibility digest and changes its scope digest, and vice versa.
  - `openspine-schemas/tests/responsibility_contract.rs::compatibility_epoch_and_scope_key_move_independently` and `::two_accounts_and_two_targets_cannot_form_one_pattern`.

## 2. Bind a reviewed scope to the standing-rule manifest

- [x] Extend `StandingRuleManifest` with the required dimension set, the reviewed value per dimension, and the derived `reviewed_scope_digest`, plus the bound `compatibility_digest`.
- [x] Derive the stored digest from the stored values so the two cannot drift apart; treat a persisted disagreement as an invalid scope that fails closed.
  - `ReviewedScopeBinding::derive_from` derives the digest from the scope's required-dimension values; a persisted disagreement surfaces as the canonical `ScopeComparison::InvalidReviewedScope`, asserted end to end by `corrupt_persisted_binding_fails_closed_as_invalid_scope`.
- [x] Reject a scope binding that carries a digest inconsistent with its values, and one that omits a required dimension, before the rule is ever persisted as active.
  - **Digest/value consistency** stays in `StandingRuleManifest::validate` (`binding_is_valid`) — the check a self-contained manifest can perform without a catalog. A persisted disagreement additionally fails closed at match time as `ScopeComparison::InvalidReviewedScope`.
  - **Required-dimension completeness** runs at **activation**, the point the standing-rules spec names ("a rule missing any required dimension MUST be rejected **before activation**") and the first point that has the catalog descriptor in scope. `store::standing_rules::scope_binding_rejection` reads the canonical `required_scope_dimensions_for(action)` — the same descriptor table the catalog is assembled from — and refuses both a binding omitting any required dimension and a rule with no binding at all for an action whose descriptor declares them.
  - Both activation entry points are guarded before their transaction opens, so the durable `standing_rule.scope_binding_rejected` audit survives the refusal: `Store::activate_standing_rule` and `Store::commit_artifact_activation` each call `reject_incomplete_scope_binding`, and `activate_standing_rule_in_tx` repeats the check as defense in depth so no path can persist an active row.
  - This is what keeps the store's contents honest: previously an incomplete binding was caught only at match time by the scope-key pre-filter, leaving a malformed rule sitting in the store looking active while being silently unmatchable.
  - Tests, one per entry point plus the no-binding case: `activation_refuses_a_binding_that_omits_a_required_dimension` (`activate_standing_rule`; omitted `Counterparty`, refusal names the dimension, no active row, one audit row), `artifact_activation_commit_refuses_an_unbound_scope_rule` (`commit_artifact_activation`; no rule row, no learned-artifact row, one audit row), and `unbounded_legacy_rule_cannot_admit_scope_bound_action` / `delegated_email_draft_without_resolved_scope_is_refused_before_dispatch` (no binding at all).
- [x] Migrate the standing-rule store schema and persist the scope binding; keep the existing quota/rate/expiry/dark-window columns unchanged.
  - v5 adds only two nullable columns via the idempotent additive lane; `git diff` shows no change to `quota_max`, `quota_window_secs`, `rate_max`, `rate_window_secs`, `expires_after_secs`, `dark_window_timeout_secs`, or `dark_window_default`. Covered by `store::migration_scoped_tests::v5_standing_rule_scope_binding_columns_added_and_dropped`.

## 3. Construct the resolved context at the kernel boundary

- [x] Build `ResolvedActionContextInput` from the kernel's own connector instance, account role/identity, canonical target refs, bound counterparty, bound parameters, and digests — never from shell-supplied fields.
- [x] Call `ResolvedActionContext::try_new` before any standing-rule consultation and surface each `ResolvedActionContextError` as a fail-closed outcome.
- [x] Scope the wiring to `email.create_draft`, the only action with a registered implementation descriptor.
- [x] Do not add a parallel context type and do not let the generic shell dispatch path reconstruct a digest-bound context from an opaque payload.

## 4. Match exactly one compatible scoped rule

- [x] Replace single-valued `active_standing_rule_for_action` lookup with scoped matching over every rule active for the action at `now`.
- [x] Require both the compatibility epoch and the reviewed scope to match; report the exact changed dimensions on a mismatch.
- [x] Admit on exactly one match, fall back to ordinary owner approval on zero, and fail closed on two or more.
- [x] Run selection before quota/rate reservation and before dark-window timer scheduling, and bind the selected rule identity inside the existing `BEGIN IMMEDIATE` so a concurrent activation cannot swap it.
- [x] Keep budgets strictly per rule — no aggregate per-action counter.

## 5. Restore ordinary approval on drift

- [x] Stop matching a rule whose stored compatibility epoch or reviewed scope no longer equals the freshly resolved context.
- [x] Return the action to ordinary owner approval before any effect runs, never after.
- [x] Never remap a rule onto a successor connector or account; an unresolvable connector/account is a construction failure.

## 6. Reach the shared executor and map the outcome

- [x] Make scope-matched admission the third caller of the `gmail.create_draft` executor, handing it the kernel-resolved digest-bound request.
- [x] Map `EffectOutcome::Executed` to reservation finalize.
- [x] Map `EffectOutcome::DeliveryUnknown` to reservation retain with the reconciliation fence left open.
- [x] Keep `RefusedPreEffect`, `FailedAfterAttempt`, and `NoExecutor` on the existing cancel semantics, including re-arming a fired one-use token only after a successful cancel.
- [x] Leave the executor itself unchanged: every re-derivation, the pending-write fence, and the permit-before-fence ordering stay as #127 landed them.

## 7. Test the boundaries

- [x] Two different accounts and two different targets cannot form one pattern.
- [x] Two disjoint scoped rules on one action coexist, each matching only its own context, with independent budgets and no pooling.
- [x] An ambiguous overlap fails closed: no reservation row, no scheduled timer, no budget consumed, fallback to ordinary approval.
- [x] A scope mismatch consumes no budget and schedules nothing.
- [x] Mutating any bound epoch or scope dimension restores ordinary approval before the effect runs.
  - `mutated_compatibility_epoch_...` (drift epoch), `mutated_scope_dimension_...` (bound counterparty), and `a_new_thread_participant_restores_ordinary_approval_before_effect` (the thread's participant set, bound through `BoundParameters`, with `TargetDigest` binding the drafted recipient — the one drift class neither digest could previously see).
- [x] Each `EffectOutcome` drives its reservation decision, with `DeliveryUnknown` retaining the reservation and the fence.
- [x] A persisted scope binding whose values disagree with its digest fails closed as invalid rather than matching on either half.

## 8. Document and record (authority-sensitive)

- [x] Record the two-digest decision, the fail-closed ambiguity rule, and the `EffectOutcome` → reservation mapping as a new decision-log entry (index row, full section, Change Log row).
  - D-155–D-158 in `.raw/openspine-decision-log.md`: index rows at lines 150-153, full sections at 3673/3695/3717/3739, Change Log row at 3817.
- [x] Update the four capability specs and confirm every requirement already present in a pre-seeded spec is carried as `MODIFIED`.
  - `specs/{standing-rules,responsibility-contract,gate-action-api,digest-bound-draft-approval}/spec.md`, each carrying `## MODIFIED Requirements`; `openspec validate … --strict` passes.
- [x] Verify the authority boundary end to end: a scoped rule remains a composition input, every admitted task still mints a fresh task grant and crosses `gate()`, and no shell input can supply or widen a reviewed scope dimension.
  - Composition input, not authority: scoped selection sits strictly downstream of `gate()` (`api/actions.rs` calls `gate()` before the consult chain, and the scoped arm is reachable only while `decision` is `ApprovalRequired`), so a rule can narrow when approval is required but can never widen a grant. Proven by `scoped_rule_cannot_override_a_gate_denial`.
  - Fresh grant per task: grant minting is untouched by this change (no pipeline/lane file is modified); the scoped path takes `&TaskGrant` read-only and every request still crosses `gate()` with that grant.
  - No shell-supplied or shell-widened dimension: proven by `shell_payload_cannot_supply_or_widen_a_reviewed_scope_dimension`, which sends a payload carrying forged `target_ref`, `target_digest`, `account_identity_digest`, `account_role`, `connector_instance_id`, `counterparty_identity_id`, `workflow_id`, and `task_shape_digest` and asserts the sealed scope key is byte-identical to the honest request's, with target from the selection token, counterparty from the briefcase, workflow from the grant, and `bound_parameters` empty.
- [x] Verify the audit boundary: the admitting rule id, rule version, and both digests are recorded with the admission, and an ambiguous-overlap refusal leaves durable owner-actionable evidence.
  - `scoped_rule_admits_draft_through_production_path_and_finalizes_reservation` reads back the `action.gated` row and asserts it carries the rule id, the rule version, the bound compatibility epoch, and the reviewed-scope digest.
  - `ambiguous_overlap_fails_closed_and_consumes_no_budget` asserts exactly one durable `standing_rule.ambiguous_scope_overlap` audit row alongside zero reservations, zero timers, and no provider write.
- [x] Confirm no #127 boundary regressed: `NoExecutor` stays an opaque `500 {"error": "internal_error"}`, the non-effect stub allowlist stays at seven catalogued READ ids, and the executor still takes its write permit before the pending-write fence.
  - Opaque 500: `api/actions.rs` still maps `DispatchError::NoExecutor` to `(INTERNAL_SERVER_ERROR, {"error": "internal_error"})`; asserted at HTTP level by `api::dispatch_tests::unregistered_effect_actions_fail_closed_without_stub`, and the generic-path `NoExecutor` lane is still exercised by `api::effect_executor_tests::execution_backed_readiness_and_no_executor_summaries_are_distinct`.
  - Stub allowlist: `action_catalog.rs` `with_non_effect_stub([…])` is unmodified and still lists exactly seven READ ids (`memory.read:owner_preferences_limited`, `memory.read:writing_preferences_scoped`, `email.read_inbox`, `email.read_thread:unselected`, `email.read_attachment`, `filesystem.host_read`, `vault.secret_read`), pinned by `action_catalog::tests::non_effect_stub_allowlist_is_explicit_and_fails_closed`.
  - Permit before fence: `pipeline/approval_draft.rs` still calls `admit_connector_write` before `insert_pending_draft_write`; the ordering is pinned by `pipeline::tests::approval_draft_reconcile_tests` ("the fence must be recorded only after the write permit is held"). The executor is byte-unchanged in this diff.
