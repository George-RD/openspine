# Tasks

## 1. Add the reviewed-scope key to the resolved context

- [ ] Add `ResolvedActionContext::reviewed_scope_digest()` computed over exactly the values named by `required_scope_dimensions`, sealed with `digest_of(&serde_json::json!({…}))` in the dimension order used by `validate_required_dimensions`.
- [ ] Add a per-dimension reviewed-value accessor so a caller can persist and compare individual dimensions, not only the digest.
- [ ] Keep `compatibility_digest()` byte-identical: it remains the drift epoch and MUST NOT gain an instance axis.
- [ ] Prove the two digests are independent — a context differing only in an instance axis keeps its compatibility digest and changes its scope digest, and vice versa.

## 2. Bind a reviewed scope to the standing-rule manifest

- [ ] Extend `StandingRuleManifest` with the required dimension set, the reviewed value per dimension, and the derived `reviewed_scope_digest`, plus the bound `compatibility_digest`.
- [ ] Derive the stored digest from the stored values so the two cannot drift apart; treat a persisted disagreement as an invalid scope that fails closed.
- [ ] Extend `StandingRuleManifest::validate` to reject a scope binding that omits a required dimension or carries a digest inconsistent with its values.
- [ ] Migrate the standing-rule store schema and persist the scope binding; keep the existing quota/rate/expiry/dark-window columns unchanged.

## 3. Construct the resolved context at the kernel boundary

- [ ] Build `ResolvedActionContextInput` from the kernel's own connector instance, account role/identity, canonical target refs, bound counterparty, bound parameters, and digests — never from shell-supplied fields.
- [ ] Call `ResolvedActionContext::try_new` before any standing-rule consultation and surface each `ResolvedActionContextError` as a fail-closed outcome.
- [ ] Scope the wiring to `email.create_draft`, the only action with a registered implementation descriptor.
- [ ] Do not add a parallel context type and do not let the generic shell dispatch path reconstruct a digest-bound context from an opaque payload.

## 4. Match exactly one compatible scoped rule

- [ ] Replace single-valued `active_standing_rule_for_action` lookup with scoped matching over every rule active for the action at `now`.
- [ ] Require both the compatibility epoch and the reviewed scope to match; report the exact changed dimensions on a mismatch.
- [ ] Admit on exactly one match, fall back to ordinary owner approval on zero, and fail closed on two or more.
- [ ] Run selection before quota/rate reservation and before dark-window timer scheduling, and bind the selected rule identity inside the existing `BEGIN IMMEDIATE` so a concurrent activation cannot swap it.
- [ ] Keep budgets strictly per rule — no aggregate per-action counter.

## 5. Restore ordinary approval on drift

- [ ] Stop matching a rule whose stored compatibility epoch or reviewed scope no longer equals the freshly resolved context.
- [ ] Return the action to ordinary owner approval before any effect runs, never after.
- [ ] Never remap a rule onto a successor connector or account; an unresolvable connector/account is a construction failure.

## 6. Reach the shared executor and map the outcome

- [ ] Make scope-matched admission the third caller of the `gmail.create_draft` executor, handing it the kernel-resolved digest-bound request.
- [ ] Map `EffectOutcome::Executed` to reservation finalize.
- [ ] Map `EffectOutcome::DeliveryUnknown` to reservation retain with the reconciliation fence left open.
- [ ] Keep `RefusedPreEffect`, `FailedAfterAttempt`, and `NoExecutor` on the existing cancel semantics, including re-arming a fired one-use token only after a successful cancel.
- [ ] Leave the executor itself unchanged: every re-derivation, the pending-write fence, and the permit-before-fence ordering stay as #127 landed them.

## 7. Test the boundaries

- [ ] Two different accounts and two different targets cannot form one pattern.
- [ ] Two disjoint scoped rules on one action coexist, each matching only its own context, with independent budgets and no pooling.
- [ ] An ambiguous overlap fails closed: no reservation row, no scheduled timer, no budget consumed, fallback to ordinary approval.
- [ ] A scope mismatch consumes no budget and schedules nothing.
- [ ] Mutating any bound epoch or scope dimension restores ordinary approval before the effect runs.
- [ ] Each `EffectOutcome` drives its reservation decision, with `DeliveryUnknown` retaining the reservation and the fence.
- [ ] A persisted scope binding whose values disagree with its digest fails closed as invalid rather than matching on either half.

## 8. Document and record (authority-sensitive)

- [ ] Record the two-digest decision, the fail-closed ambiguity rule, and the `EffectOutcome` → reservation mapping as a new decision-log entry (index row, full section, Change Log row).
- [ ] Update the four capability specs and confirm every requirement already present in a pre-seeded spec is carried as `MODIFIED`.
- [ ] Verify the authority boundary end to end: a scoped rule remains a composition input, every admitted task still mints a fresh task grant and crosses `gate()`, and no shell input can supply or widen a reviewed scope dimension.
- [ ] Verify the audit boundary: the admitting rule id, rule version, and both digests are recorded with the admission, and an ambiguous-overlap refusal leaves durable owner-actionable evidence.
- [ ] Confirm no #127 boundary regressed: `NoExecutor` stays an opaque `500 {"error": "internal_error"}`, the non-effect stub allowlist stays at seven catalogued READ ids, and the executor still takes its write permit before the pending-write fence.
