# Tasks

## 1. Rebase and re-derive before implementation

- [x] Do not begin implementation until proposal review and rebase onto `origin/main` containing archived #129/#133.
- [x] Re-read affected canonical specs and re-derive every `MODIFIED` body; do not duplicate `GmailDraftScope`, `thread_match`, `evidence_summary`, catalog/judge validation, or any #135 wrapper repair.
- [x] Run `node_modules/.bin/openspec validate ship-recurring-gmail-draft-proof --strict` before implementation and after every spec revision.

## 2. Erased-counterparty admission and sweep

- [x] Add fail-closed store query `is_counterparty_erased` against `erased_counterparties`; do not inspect filesystem/key-ring/ledger in the matcher.
- [x] Check it after unresolved-counterparty rejection in `resolve_scoped_admission`, at the fired-pending mediation entry, and inside `BEGIN IMMEDIATE` transactions before scoped rule selection/reservation or fired-token claim/reservation. The two pre-transaction sites are proven by production-entering tests; the in-transaction rechecks are landed and read-verified but **UNPROVEN** by such a test, because the pre-transaction check refuses first on the same identity (see the census rows below).
- [x] Extend the erasure transaction to sweep generic reviewed-scope `Counterparty` values for all persisted standing rules, including owner-review-created rows without `learned_artifacts` provenance; revoke/stale affected live state atomically and idempotently.
- [x] Preserve ledger-first/post-commit key cleanup and explicitly do not erase plaintext briefcases, pending payloads, `SYSTEM_SCOPE`, or owner-review rows.

## 3. Typed approval evidence and miner cutover

- [x] Persist optional typed context metadata on eligible approval audit events, deriving `ReviewedActionScope::derive(&resolved_context)?.context_class_digest()` for the evidence/grouping key and retaining `ResolvedActionContext::reviewed_scope_digest()` separately as the standing-rule scope binding; include the metadata in the existing audit hash pre-image and make historical/missing-context rows ineligible.
- [x] Derive request digest only from `ResolvedActionContext::task_shape_digest()`; preserve separate target/payload digests and exclude body, recipients, message IDs, timestamps, and decision event id.
- [x] Replace raw miner row count with `(action_id, context_class_digest)` grouping, `BTreeSet<decision_event_id>` deduplication, and `DelegationEvidence::repeated_approvals`; remove `reviewed_scope = None` and free-text copy.
- [x] Rewrite the scheduled repeated-approval tests `scheduled_reflection_miner_tick_mines_repeated_approval`, `scheduled_reflection_miner_duplicate_decision_event_is_not_repeated`, `scheduled_reflection_miner_two_context_classes_do_not_form_one_pattern`, and `scheduled_reflection_miner_request_shape_mismatch_is_rejected`; prove duplicate-only no candidate, cross-context no pattern, request-shape mismatch rejection, payload-only variation validity, and derived `2 matching owner approvals` copy.

## 4. Pending-write retry fence

- [x] Add protected-reference stable request fingerprint to `pending_draft_writes` with additive migration and query helper.
- [x] Read the pending fence after kernel context/target resolution and before scoped consultation/reservation or Gmail execution. A hit falls back to owner review, performs no write, and reserves no budget.
- [x] Keep `DeliveryUnknown` pending and known outcomes resolving; make no exactly-once claim.
- [x] Add a kernel action-API retry test that seeds pending state, asserts zero executor calls/ordinary approval, and fails if the read is removed; add explicit-resolution clearing coverage.

## 5. Responsibility receipt and catalog boundary

- [x] Emit an optional scoped effective-Allow receipt naming rule id/version, canonical target refs, and post-reservation quota/rate remaining; keep owner-review lifecycle receipts free of external-effect claims.
- [x] Add caller-level owner-path tests for receipt contents, fallback without receipt, and DeliveryUnknown/failure truthfulness.
- [x] Add `email.send` to catalog-owned non-delegable data and characterize all readers: exact catalog cardinality, worker-grant attenuation rejection, catalog/judge refusal before review, and the inverted overlay-export requirement (which must still accept only the two overlay ids). Do not add manifest-specific validation.
- [ ] Add a kernel action API regression proving `email.create_draft` never falls back to action-keyed `consult_standing_rule_gate`. — OPEN: no named kernel action-API regression test exists; the production guard is present but this control remains unproven.

## 6. Reachability census and mutation killers


Complete this table before ticking any guard task. Production callers exclude `*_tests.rs`, `/tests/`, and `_tests`; a blank second or third column is dead/unproven and MUST be reported. The test MUST enter at the production caller or higher, not call the guard directly.

| GUARD SITE | PRODUCTION CALLER | TEST THAT ENTERS AT OR ABOVE THAT CALLER |
| --- | --- | --- |
| `is_counterparty_erased` + normal admission check | `api/scoped_admission.rs::resolve_scoped_admission` from `api/actions.rs::post_actions` | `erased_counterparty_scoped_admission_falls_back_to_owner_approval` via kernel action API |
| Pre-existing `CounterpartyRef::Unresolved` guard (`scoped_admission.rs:208-218`) | `api/actions.rs:516-520` -> `resolve_scoped_admission` | `unresolved_counterparty_falls_back_via_action_api` via kernel action API with an actually `Unresolved` briefcase, asserting this guard's own audit reason; pre-existing #128 gap closed incidentally. Killing mutation: deleting the guard block still yields `ApprovalRequired` (the scope no longer matches), so the test binds on the reason string — a reason-free assertion survives and is not evidence. |
| Fired-entry marker check | `api/actions.rs:543-595` fired-pending branch in `post_actions` | **UNPROVEN / latent:** no caller-level test can enter this branch under the shipped empty Allowlist; report rather than add a direct-wrapper test |
| Scoped reservation transactional recheck | `store/standing_rules_scoped.rs:137-144` in `consult_and_reserve_scoped_rule`, from `api/scoped_admission.rs::consult_scoped_rule` (`api/actions.rs:621`) | **UNPROVEN / latent:** the recheck is inside the `BEGIN IMMEDIATE` that selects and reserves, but no production-entering test reaches it — the pre-transaction check at `scoped_admission.rs:219-232` refuses first on the same identity, and `Counterparty` is a required scope dimension so the two can never disagree without a mid-flight commit. Reaching it needs a marker committed between the two reads, which has no test seam today. Report rather than credit the early-check test twice. |
| Fired-token transactional recheck | `store/standing_rules_fired_token.rs::consume_standing_rule_fired_pending`, from the production `api/actions.rs:586-595` caller | **UNPROVEN / latent:** no production-entering test is possible while minting is inert at `api/actions.rs:665-671` and the Allowlist is empty; report, do not add a direct-wrapper test |
| Generic standing-rule erasure sweep | `main.rs:303` → `reconcile_overlay_terminal_erasures` → `finish_local_erasure` → `mark_learned_artifacts_erased` — a real, unconditional production caller executed at every startup. The loop body runs for ids in an imported continuity ledger; local production does not originate one today because the sole `record_terminal_erasure` caller is the dead `erase_counterparty`, while overlay restore propagates imported ids without originating them. **Wired and unconditionally invoked at startup; reachable via an imported continuity ledger; local origination absent.** | `startup_terminal_erasure_revokes_scoped_rule_before_reuse` (`overlay_startup_tests/integrity.rs`), seeding the terminal ledger via `OverlayOperations::record_terminal_erasure` and entering through `validate_startup_and_reconcile_overlay_terminal_erasures` — the same production startup path — so the sweep is exercised through the real production caller, not the `#[allow(dead_code)] erase_counterparty` wrapper and not a direct `erased_counterparties` insert. **Residual:** local production has no `record_terminal_erasure` originator; restore can import ids without originating them. The reachability condition for local origination is an owner erasure command. |
| Pending-write read fence — kernel action path | `pipeline/approval_draft.rs::create_approved_draft` and scoped `post_actions` -> shared Gmail executor | `pending_delivery_unknown_fences_scoped_retry_before_reservation` via kernel action API |
| Pending-write read fence — Telegram approval callback | `pipeline/approval.rs::handle_draft_approval_callback` via `pipeline/mod.rs:493-500` -> shared Gmail executor | **UNPROVEN / latent:** no named callback-entering test yet; add one before claiming complete end-to-end pending-fence coverage |
| Miner grouping/dedup | scheduled miner tick -> `reflection_miner_runtime/scheduled.rs` | `scheduled_reflection_miner_tick_mines_repeated_approval` plus permanent asymmetric killer `asymmetric_rows_prove_context_grouping` via scheduled runtime route; exact one-row and 2x2 mutation evidence is recorded below. |
| Miner owner-review origination/eval binding | `reflection_miner_runtime::dispatch_reflection_proposal` after the shared `artifact.propose` replay/risk-judge gate and before `persist_owner_review` | `miner_proposal_missing_verdict_refuses_without_review_or_activation`, `miner_proposal_mismatched_verdict_refuses_without_review_or_activation`, `miner_proposal_non_review_required_refuses_without_review_or_activation`, and `miner_proposal_stale_verdict_refuses_without_review_or_activation` via the scheduled runtime route; each dispatch control asserts no `OwnerReviewRequest` row and no activation. `miner_review_approval_denied_verdict_refuses_without_activation` and `miner_approval_refuses_review_proposal_digest_mismatch` covers approval-time absent/mismatched/state mutation and asserts no activation. |
| Legacy owner-review digest compatibility | `OwnerReviewRequest::calculate_binding_digest` during owner-review deserialization | `legacy_owner_review_binding_digest_is_stable_for_schema_additions` — guard is landed; remove-`skip_serializing_if` mutation is pending the evaluation-binding field |
| Resume lifecycle refusal mapping | `pipeline/owner_review_decision.rs::commit_lifecycle` via `handle_terminal_message` | `terminal_review_resume_refusal_is_truthful` through `/review ... resume`; production proof is connector-unavailable Resume and asserts paused rule unchanged, no successful committed receipt, and durable refusal event. Killing mutation: `ResumeOutcome::Refused(_) => true` (old unchanged-replay shape) dies on the unavailable arm with owner-facing `Resume (replay: unchanged)`; the temporary `=> false` transition-shape mutation also died at the same assertion. Expiry, supersession, invalid scope, and scope drift remain direct-helper coverage under the shared mapping. |
| Pause lifecycle refusal mapping | `store/standing_rules_lifecycle.rs::pause_standing_rule` via `handle_terminal_message` | `terminal_review_pause_refusal_is_truthful` through the owner terminal route; production proof asserts `AlreadyPaused` unchanged replay and typed `needs_review`/non-active refusal with unchanged rule, no successful receipt, and durable refusal event. Killing mutation: `PauseStandingRuleOutcome::Refused => true` dies on the needs-review/non-active arm with owner-facing replay/transition output; the AlreadyPaused branch remains a legitimate replay. |
| Responsibility receipt | `api/actions.rs:629-640` scoped effective-Allow response/effect assembly | `scoped_allow_returns_responsibility_receipt` via kernel action API |
| Catalog non-delegable `email.send` | `openspine-authority/src/worker_grant.rs:99-102` worker-grant attenuation; `overlay_eval_gate/eval_input.rs:213-216` -> `overlay_eval_gate/judge.rs:98-106` catalog/judge refusal; `api/overlay_export_restore.rs:64-68` inverted trusted overlay reader | `non_delegable_catalog_is_exactly_the_root_only_set` (catalog datum), `catalog_email_send_is_non_delegable` (the judge reader entered through the production `dispatch_artifact_propose` path; killing mutation: dropping `email.send` from `with_non_delegable` moves the refusal to the policy-deny axis and the test fails), `non_delegable_actions_rejected_even_if_parent_allows`, `propose_path_refuses_a_non_delegable_standing_rule_before_review_required`, and `overlay_export_restore_are_non_delegable_with_no_egress` characterize the three readers and preserve the overlay-only inverted allowlist |
- [x] Keep resolution (a): do not add an owner erasure surface in #130. The generic sweep is **wired and unconditionally invoked at startup; reachable via an imported continuity ledger; local origination absent**. The startup test must seed the imported ledger and enter through `validate_startup_and_reconcile_overlay_terminal_erasures`; keep the local owner-erasure command as the residual reachability condition, and keep the fired-token rows' separate empty-dark-window-allowlist condition visibly distinct.
- [x] Record mutation identities and results: **single-site key-only context-grouping removal is killed by `asymmetric_rows_prove_context_grouping`** (the named 2-versus-3 fixture); deleting `BTreeSet` dedup is killed by `scheduled_reflection_miner_duplicate_decision_event_is_not_repeated`. The one-row and symmetric 2x2 fixtures survive key-only removal only because the scheduled route emits the selected maximum (one row per class yields tick zero either way; equal maxima remain indistinguishable), not because context grouping is unenforced. The coupled grouping-plus-binding mutation is corroborating evidence, not the primary kill. The duplicate-only test survives dedup removal because the `DelegationEvidence::repeated_approvals` contract independently rejects duplicate event ids; `duplicate_audit_row_does_not_poison_distinct_approval_pattern` is the miner-layer killer.
### Miner grouping mutation evidence (captured before restoring production)

**Killer first:** single-site key-only context-grouping removal is killed by
`asymmetric_rows_prove_context_grouping` (2 rows in one context class versus 3
in another; the scheduled route's selected maximum makes the classes
observable).

**Survivors, with the reason:** the one-row fixture survives because one row per
class yields tick zero either way; the symmetric 2x2 fixture survives because
both classes have equal maxima. Those fixtures cannot discriminate the
grouping guard and are not evidence that it is unenforced.

**Coupled result:** removing both the context key and the per-group binding
rejection is corroborating evidence; it is not the primary single-site kill.

The named one-row fixture has one valid approval row per context class. The exact key-only mutation was:

```diff
@@ crates/openspine-kernel/src/reflection_miner_runtime/scheduled.rs:286-289
-        let group_key = (
-            action.as_str().to_string(),
-            metadata.context_class_digest.to_string(),
-        );
+        let group_key = (action.as_str().to_string(), String::new());
```

The literal one-row test output under that mutation was:

```text
running 1 test
ONE_ROW_GROUPING_TICK=0
test api::artifact_propose_tests::artifact_propose_miner_grant_tests::scheduled_reflection_miner_two_context_classes_do_not_form_one_pattern ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1095 filtered out; finished in 0.23s
```

The coupled mutation removed the same context component and the later per-group binding rejection:

```diff
@@ crates/openspine-kernel/src/reflection_miner_runtime/scheduled.rs:286-299
-        let group_key = (
-            action.as_str().to_string(),
-            metadata.context_class_digest.to_string(),
-        );
+        let group_key = (action.as_str().to_string(), String::new());
@@ crates/openspine-kernel/src/reflection_miner_runtime/scheduled.rs:297-299
-        if group.3 != binding || !group.2.insert(event.id) {
+        if !group.2.insert(event.id) {
             continue;
         }
```

Its literal output was:

```text
running 1 test
ONE_ROW_GROUPING_TICK=1

thread 'api::artifact_propose_tests::artifact_propose_miner_grant_tests::scheduled_reflection_miner_two_context_classes_do_not_form_one_pattern' (33696039) panicked at crates/openspine-kernel/src/api/artifact_propose_miner_grant_tests.rs:486:5:
assertion `left == right` failed
  left: 1
 right: 0
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
test api::artifact_propose_tests::artifact_propose_miner_grant_tests::scheduled_reflection_miner_two_context_classes_do_not_form_one_pattern ... FAILED

failures:

failures:
    api::artifact_propose_tests::artifact_propose_miner_grant_tests::scheduled_reflection_miner_two_context_classes_do_not_form_one_pattern

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1095 filtered out; finished in 0.20s

error: test failed, to rerun pass `-p openspine-kernel --bin openspine`
```

The requested 2x2 attempt used two rows in each of two classes: class A was
`thread-1`/account `1`/counterparty `11` with payload bytes `f,0`; class B
was `thread-2`/account `2`/counterparty `12` with payload bytes `1,2`.
Both classes used the same request-shape digest and distinct full reviewed
scope bindings. The scheduled route emits only the selected maximum group,
not one proposal per group. The captured outputs were:

```text
intact:   TWO_BY_TWO_TICK=1; description: 2 matching owner approvals
key-only: TWO_BY_TWO_TICK=1; description: 2 matching owner approvals
coupled:  TWO_BY_TWO_TICK=1; description: 4 matching owner approvals
```

For the key-only 2x2 run, every row first passed the metadata-vs-its-own-
binding check at `scheduled.rs:280-283`; after action-only grouping, class B
was dropped by the later `group.3 != binding` check because `group.3`
retained class A's binding. The coupled mutant removed that rejection and
pooled all four rows, which is why only its derived copy changed from `2` to
`4`. Equal maxima therefore leave key-only and intact with the same selected
two-row copy; this is why the 2x2 attempt alone was insufficient.

The follow-up asymmetric fixture retained the permanent test and made the
single kill constructible: class A had two rows and class B had three. Its
literal outputs were:

```text
intact:
ASYMMETRIC_TICK=1 FIRST=None SECOND=Some("description: 3 matching owner approvals")
test api::artifact_propose_tests::artifact_propose_miner_grant_tests::grouping_tests::asymmetric_rows_prove_context_grouping ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1096 filtered out; finished in 0.25s

key-only:
ASYMMETRIC_TICK=1 FIRST=Some("description: 2 matching owner approvals") SECOND=None
assertion `left == right` failed
  left: None
  right: Some("description: 3 matching owner approvals")
test api::artifact_propose_tests::artifact_propose_miner_grant_tests::grouping_tests::asymmetric_rows_prove_context_grouping ... FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1096 filtered out; finished in 0.21s

coupled:
ASYMMETRIC_TICK=1 FIRST=Some("description: 5 matching owner approvals") SECOND=None
assertion `left == right` failed
  left: None
  right: Some("description: 3 matching owner approvals")
test api::artifact_propose_tests::artifact_propose_miner_grant_tests::grouping_tests::asymmetric_rows_prove_context_grouping ... FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1096 filtered out; finished in 0.21s
```

A single-mutation key-only kill is therefore constructible: the intact route
selects B's three-row proposal, key-only grouping drops B and selects A's
two-row proposal, and the coupled mutant pools all five. The 2x2 attempt is
retained as evidence of the equal-maxima limitation, not substituted for the
asymmetric killer.
The six miner-authority tests remained unmodified and passed with the
permanent grouping killer (`cargo test -p openspine-kernel
artifact_propose_miner_grant_tests -- --nocapture`: `6 passed`, including the
four canonical tests
`scheduled_reflection_miner_tick_mines_repeated_approval`,
`scheduled_reflection_miner_duplicate_decision_event_is_not_repeated`,
`scheduled_reflection_miner_two_context_classes_do_not_form_one_pattern`, and
`scheduled_reflection_miner_request_shape_mismatch_is_rejected`, plus the
dedup-poisoning killer
`duplicate_audit_row_does_not_poison_distinct_approval_pattern` and the
single-site grouping killer `asymmetric_rows_prove_context_grouping`).
### Remaining mutation results

The dedup-only mutation was the exact guard reduction:

```diff
@@ crates/openspine-kernel/src/reflection_miner_runtime/scheduled.rs:300-302
-        if group.3 != binding || !group.2.insert(event.id) {
+        if group.3 != binding {
             continue;
         }
```

Its two named outcomes were:

```text
scheduled_reflection_miner_duplicate_decision_event_is_not_repeated ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1096 filtered out; finished in 0.14s

duplicate_audit_row_does_not_poison_distinct_approval_pattern:
assertion `left == right` failed
  left: 0
  right: 1
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1096 filtered out; finished in 0.13s
```

The first survives because the schema contract rejects the duplicate event
id even without miner-layer deduplication; the second dies because that same
contract rejects the duplicate-plus-distinct set, so the distinct pattern
cannot be mined.

The request-shape mutation was also run and killed:

```diff
@@ crates/openspine-kernel/src/reflection_miner_runtime/scheduled.rs:306
-            request_digest: metadata.request_digest,
+            request_digest: metadata.payload_digest.clone(),
```

`scheduled_reflection_miner_request_shape_mismatch_is_rejected` failed under
that mutation with `left: 1`, `right: 0`: both rows deliberately held the
same payload digest, so the payload-derived mutant collapsed their distinct
task-shape digests and emitted a candidate. The intact path passes with no
candidate.

The two lifecycle counterfactuals were also killed at the owner terminal
route: changing `ResumeOutcome::Refused(_)` to the old unchanged-replay
truth value produced the unavailable-arm owner-facing `Resume (replay:
unchanged)` failure, and the temporary `=> false` transition-shape mutation
failed the same `terminal_review_resume_refusal_is_truthful` assertion.
The exact typed lifecycle assertions now cover `AlreadyActive`, all five
`ResumeRefusal` variants, and `Resumed`; no pause accessor existed to remove.

- [x] Extend `reflection_miner_runtime::dispatch_reflection_proposal` through the existing `artifact.propose` path: return the shared proposal/evaluation receipt, persist both replay/risk-judge verdicts and their epochs, bind proposal kind/id/version/digest plus verdict identities into `OwnerReviewRequest`, and call `persist_owner_review` only after the explicit presence/identity/digest/`review_required` checks pass. At approval time, perform the immutable-binding comparison against the stored `ProposedArtifact` row and exact `eval_verdicts` rows inside the same `BEGIN IMMEDIATE` transaction as `commit_artifact_activation` (or fold it into that commit), before the existing stale-epoch currency path. Approve through the evaluated artifact-activation/currency path; for a miner-originated review, re-enter the same exact `artifact.propose` evaluation before persisting a Narrow replacement. Add named controls `miner_proposal_missing_verdict_refuses_without_review_or_activation`, `miner_proposal_mismatched_verdict_refuses_without_review_or_activation`, `miner_proposal_non_review_required_refuses_without_review_or_activation`, `miner_proposal_stale_verdict_refuses_without_review_or_activation`, and `miner_review_approval_denied_verdict_refuses_without_activation` and `miner_approval_refuses_review_proposal_digest_mismatch`; each dispatch refusal MUST assert no `OwnerReviewRequest` row and no activation, while approval-time failures MUST assert no activation.
- [x] Make the new miner evaluation-binding field on `OwnerReviewRequest` an `Option` with `#[serde(default, skip_serializing_if = "Option::is_none")]`; add a legacy review fixture test asserting its binding digest is byte-identical before and after the schema change, with no `null` field serialized.
- [x] Make Resume and Pause refusals truthful: distinguish `Resumed`, `AlreadyActive` replay, and `Refused` outcomes for Resume, and `Paused`, `AlreadyPaused` replay, and `Refused` outcomes for Pause; map `Refused` to `LifecycleRefused` before `commit_owner_review_decision`, preserve legitimate replays as unchanged receipts, and refuse before committing a successful owner-review receipt for expiry, drift, invalid scope, unavailable, supersession, needs-review, revoked, or other ineligible non-replayed states. Keep the rule unchanged on refusal, record the durable refusal event for refused intents, and exercise the terminal-route tests `terminal_review_resume_refusal_is_truthful` and `terminal_review_pause_refusal_is_truthful` (including the already-paused replay).

## 7. Specs, verification, capability sequencing, residuals

- [ ] Only after archive, verify runtime ids are archived non-integer ids, regenerate the deferred map end state (`progressive-delegation: proof_in_progress`; selected `recurring-gmail-draft: shipped`) with `node scripts/capability-map.mjs --write`, and never hand-edit roadmap markers. Do not claim `wired_into_lyra` until a later product-surface change adds Lyra artifacts and capability-level positive/fallback evidence. — OPEN: archive and post-archive capability-map regeneration are intentionally deferred to the landing ceremony.
- [x] Add positive owner-path and fallback/control tests for capability-map evidence, naming `scoped_allow_returns_responsibility_receipt` as `positive_effect`, `erased_counterparty_scoped_admission_falls_back_to_owner_approval` as `fallback`, and `pending_delivery_unknown_fences_scoped_retry_before_reservation` as `control` (plus the unresolved/denied controls); record their real test paths in the selected proof after implementation, then run targeted tests, `./scripts/check.sh ship-recurring-gmail-draft-proof`, and strict validation.
- [ ] Produce a before/after scenario and body diff against the four canonical requirement files modified by this change (`responsibility-contract`, `reflection-miner`, `standing-rules`, and `digest-bound-draft-approval`); every pre-existing scenario and requirement body MUST remain verbatim unless this delta intentionally changes that requirement, with only the new dedup/grouping, erasure, pending-fence, receipt, catalog, and evaluation-binding scenarios and deliberate body additions recorded. — OPEN: no separate before/after scenario-and-body-diff record is stored in the change artifacts.
- [x] Final report lists briefcase plaintext, `SYSTEM_SCOPE`/pending protected payloads, owner-review rows, miner-vs-owner erasure provenance split, external-ledger/SQLite/filesystem boundary, and latent #135 evidence: `openspine-change-sequence.md:128`, `api/actions.rs:665-671` `None,None`, `api/actions.rs:586-595`, `pipeline/standing_rule_timer.rs:378-389`, empty allowlist `action_catalog_contracts.rs:54-60`, and `email.create_draft` policy `action_catalog_data.rs:84`. Do not edit the ledger.
