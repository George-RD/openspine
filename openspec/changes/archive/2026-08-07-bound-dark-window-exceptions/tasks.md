# Tasks

## 1. Make the exception allowance reviewable data

- [x] Add `max_pending_exceptions` to `DarkWindowConfig`, defaulting to 1, serialized so the owner review object shows it.
- [x] Extend `StandingRuleManifest::validate` to require `1 <= max_pending_exceptions <= ` a small hard maximum.
- [x] Round-trip the field through activation and every `DarkWindowConfig` reconstruction so the persisted rule and the manifest cannot disagree.

## 2. Enforce the cap atomically in the scheduling transaction

- [x] Count nonterminal (`resolved_at IS NULL`) pending rows for the exact `(rule_id, rule_version)` inside the existing `BEGIN IMMEDIATE` transaction.
- [x] Evaluate same-request deduplication before the count so an idempotent repeat never consumes a slot.
- [x] Refuse at the limit before inserting anything: no pending row, no timer, no scheduled audit, no orphan state.
- [x] Return a typed scheduled/suppressed outcome instead of a bare `Option<timer_id>`.
- [x] Retire or repurpose the dead, non-version-aware `pending_dark_window_count` rather than leaving a second counting path beside the real one.

## 3. Report suppression as an ordinary approval

- [x] Distinguish scheduled from suppressed in the gate consultation outcome.
- [x] On suppression keep `ApprovalRequired`, reserve no quota or rate, schedule no timer, and send no owner notification.
- [x] Keep the caller's response indistinguishable from "no dark window configured" — no new capacity-state field.

## 4. Bind the pending exception to the reviewed context

- [x] Persist `reviewed_scope_digest` and `compatibility_digest` on the pending row when a scoped rule schedules an exception; nullable for a rule with no scope binding.
- [x] Require the stored digests to equal the freshly resolved context's before a fired token may be consumed, alongside every existing predicate.
- [x] Leave the request fingerprint and its token binding unchanged — it answers a different question and must keep answering it.

## 5. Account for exceptions separately from quota

- [x] Record each fired exception under a distinct audit class.
- [x] Count the allowance per `(rule_id, rule_version)`, never pooled across rules or reviewed scopes.
- [x] Do not refresh `last_used_at` on a fired exception; still evaluate the drift trigger.

## 6. Stale open exceptions on every lifecycle change

- [x] Add a staleness writer scoped to `(rule_id, rule_version)` that resolves unresolved pending rows as `stale`.
- [x] Invoke it in the same transaction as revoke, expiry/lapse, the `needs_review` transition, and a version bump.
- [x] Confirm the claim path already treats `stale` as terminal, and that recovery excludes stale rows and never re-opens a terminal slot.

## 7. Give the communication Allow prohibition enforcing code

- [x] Add a fail-closed, catalog-declared dark-window Allow eligibility contract; no action is eligible by default.
- [x] Refuse activation of a rule whose dark-window default is Allow for an ineligible action, before the transaction opens, with durable evidence.
- [x] Guard both activation entry points, as the reviewed-scope check does.

## 8. Test the boundaries

- [x] Many distinct over-budget requests varying payload, grant, and chat id leave exactly one live pending exception at the default cap.
- [x] Concurrent requests for the final slot produce exactly one winner.
- [x] The same request repeated stays idempotent and consumes no second slot.
- [x] A distinct request at the cap stays `ApprovalRequired` with no row, no timer, no budget, and no reported default.
- [x] A resolved exception frees its slot; a stale one does too.
- [x] A fired token whose reviewed scope or compatibility epoch drifted grants no authority.
- [x] A fired exception is audited distinctly, is one-use, and does not refresh the lapse clock.
- [x] Revoke, expiry, `needs_review`, and a version bump each stale open exceptions before a timer can claim them.
- [x] Activation refuses an Allow default for an action the catalog does not declare eligible.
- [x] At least one test drives the cap through the real production admission path, not a store method directly.

## 9. Document and record (authority-sensitive)

- [x] File a tracking issue for the `rate_limited_write_admission_is_refused_without_a_fence_row` timing flake, which is the only deterministic pin on #127's permit-before-fence ordering (openspine#163).

- [x] Record the bounded-exception decisions as new decision-log entries (index row, full section, Change Log row, `---` separators), starting at D-159.
- [x] Update the two capability specs and confirm every requirement already present in a pre-seeded spec is carried as `MODIFIED`.
- [x] Verify the authority boundary: silence never creates more authority than the reviewed bound, the task grant remains the upper bound, and the fired token stays one-use and scope-bound.
- [x] Confirm no #128 boundary regressed: the scope key stays `reviewed_scope_digest`, `compatibility_digest` stays byte-identical, exactly-one matching and fail-closed ambiguity are unchanged, and `DeliveryUnknown` still retains its reservation as `reserved`.
