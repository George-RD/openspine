# Mine and match reusable authority by scope

## Dependencies

- `define-responsibility-contract` (archived, **HARD**): supplies `ResolvedActionContext`, `ReviewedScopeDimension`, and the D-146 two-axis descriptor/implementation catalog this change consumes. The type is complete and tested but has **zero kernel callers** today.
- `unify-approved-and-delegated-effect-execution` (#127, archived, **HARD**): registered the `gmail.draft.v1` implementation descriptor, without which `ResolvedActionContext::try_new` fails `MissingImplementationDescriptor` for every action. It also left this change two explicit obligations, recorded in D-153.
- `bound-dark-window-exceptions` (#131) and `ship-recurring-gmail-draft-proof` (#130) depend on the scoped rule this change introduces.
- Canonical decisions: D-146, D-107, AD-036, AD-120, plus D-153 for the two inherited deferrals.

This change affects **OpenSpine core**, not Lyra product surfaces. It affects **runtime authority**: it introduces the first admission path where a reviewed scope, rather than a per-instance owner approval, admits an effect. It does not add connector access, external communication, or system-operations authority.

## Problem/Context

A standing rule today binds exactly one thing: an `ActionId`. `StandingRuleManifest` carries `id`, `schema_version`, `version`, `lifecycle_state`, `action_id`, `description`, `quota`, `rate`, `expires_after_secs`, and `dark_window` — and nothing else. Lookup matches that single key: `active_standing_rule_for_action(&ActionId, now)` returns at most one rule.

That is too coarse to be authority. "Draft emails" is not a reviewable responsibility; "draft replies on *this* Gmail account, to *this* counterparty, on *this* thread" is. With only an action key, one approved rule silently covers every account, every counterparty, and every target the action can reach, and two genuinely different responsibilities that happen to share an action collapse into one budget. AD-036's authority-equivalence classes and D-146's reviewed-scope contract both require the opposite: the matcher picks *within* an authority footprint, never across one.

The kernel already has the missing half. `ResolvedActionContext` (`crates/openspine-schemas/src/resolved_context.rs`) is a sealed 28-field context class built from catalog-selected semantics, implementation readiness, and catalog-owned effect metadata; `ReviewedScopeDimension` enumerates the generic axes; `validate_required_dimensions` fails closed on a missing required dimension. None of it is reachable from the kernel: `ResolvedActionContext` is referenced only by its own module and two schema test files.

Two further gaps, inherited from #127 and recorded in D-153:

1. The generic shell dispatch path receives `payload: Option<&serde_json::Value>`, not an `&ActionRequest`, so it cannot supply the `payload_ref`, `target_ref`, and `target_digest` the Gmail executor re-derives against. It fails closed with `DispatchError::NoExecutor`. Constructing that resolved, digest-bound context is this change's core work.
2. `EffectOutcome` is not wired to reservation lifecycle. The only reservation decision today is `NoExecutor` → cancel. `Executed` → finalize and `DeliveryUnknown` → retain-and-fence do not exist, because before this change no executor caller held a standing-rule reservation.

## Proposed Solution

1. **Construct the resolved context at the kernel boundary.** Build `ResolvedActionContext` from the kernel's own connector/account/target/counterparty resolution before consulting any standing rule, and carry it — not a shell-supplied payload — into admission. The shell supplies an intent; the kernel seals the trusted scope. Scope the proof to `email.create_draft`, the first and currently only action for which `try_new` can succeed.

2. **Separate the scope key from the drift epoch.** `compatibility_digest()` is computed over declaration axes only — descriptor/implementation/executor/resolver ids and versions, effect destination, required scope dimensions, egress class, output channels — and deliberately excludes every instance axis. It is therefore the **drift epoch**, not a scope key: matching on it would collide two different accounts into one pattern, which this change's own invariant forbids. Add a second, distinct `reviewed_scope_digest()` computed over exactly the values named by `required_scope_dimensions`, sealed the same way.

3. **Bind standing rules to reviewed scopes.** Extend `StandingRuleManifest` with the reviewed scope: the required dimensions, the individual reviewed values for each, and the derived `reviewed_scope_digest`. Storing the values (not the digest alone) is what lets comparison report the *exact* changed dimensions, which `responsibility-contract` already requires, and lets an owner narrow one dimension without invalidating the rest. Store the compatibility epoch alongside it.

4. **Match exactly one rule, before any budget moves.** Replace single-valued action-keyed lookup with scoped matching over the rules active for an action. Exactly one compatible rule admits. Zero matches falls back to ordinary owner approval. **Two or more matches fail closed** — an ambiguous overlap is an unreviewed authority question, not a tie to break. Matching completes before quota/rate reservation and before any dark-window timer is scheduled, so neither a mismatch nor an overlap can consume budget or mint a pending exception.

5. **Restore ordinary approval on drift.** A changed descriptor, implementation, policy, executor, connector instance, account identity, counterparty, workflow, or task shape changes the bound digest. The rule stops matching and the action returns to ordinary approval **before** any effect — never a silent remap to a successor connector or account.

6. **Wire `EffectOutcome` to the reservation lifecycle.** `Executed` finalizes the reservation; `DeliveryUnknown` retains it and leaves the reconciliation fence open, because releasing budget for a write that may have landed would under-count real effects; `RefusedPreEffect` and `FailedAfterAttempt` keep #127's cancel semantics. This closes the second D-153 deferral.

## Acceptance Criteria

- Two different accounts, or two different targets, cannot form one pattern: contexts differing in any bound instance dimension produce different `reviewed_scope_digest` values and cannot be admitted by each other's rule.
- Disjoint scoped rules coexist for one action: two rules on `email.create_draft` with non-overlapping reviewed scopes are both active, each matches only its own context, and each holds its own independent quota and rate budget with no pooling between them.
- An ambiguous overlap fails closed and consumes no budget: when two active rules both match one resolved context, admission is refused, no reservation row exists, no dark-window timer is scheduled, and the action falls back to ordinary owner approval.
- A scope mismatch consumes no budget: a context matching no active rule reserves nothing and schedules nothing.
- Changed context restores ordinary approval before effect: mutating any bound epoch or scope dimension makes the previously matching rule stop matching, and the effect does not run under the stale rule.
- A resolved, digest-bound context reaches the shared `gmail.create_draft` executor from scope-matched admission — the third caller — and its `EffectOutcome` drives the reservation decision: `Executed` finalizes, `DeliveryUnknown` retains with the fence open, refusal and post-attempt failure cancel.
- `node_modules/.bin/openspec validate mine-and-match-reusable-authority-by-scope --strict` passes, and every delta requirement whose header already exists in its pre-seeded capability spec is carried as `## MODIFIED Requirements`.

## Invariant

**No fuzzy widening, no cross-account reuse, no cross-target evidence aggregation, and no budget pooling between responsibilities sharing an action.** Matching is exact over sealed kernel-resolved values. There is no similarity threshold, no nearest-match, and no "close enough" dimension. A rule reviewed for one account never admits another; approval evidence gathered on one target never supports a different target; and two responsibilities that share an `ActionId` never share a quota or rate window.

## Out of Scope

- Re-opening boundaries #127 settled. `DispatchError::NoExecutor` stays an opaque `500 {"error": "internal_error"}`, uniform with every other dispatch failure, so a semi-trusted shell cannot enumerate which catalogued actions are unwired. The non-effect stub allowlist stays closed at its seven catalogued READ ids. The executor keeps taking its write permit before recording the pending-write fence.
- Actions other than `email.create_draft`. It is the only action whose implementation descriptor is registered, so it is the only one for which a resolved context is constructible. Extending the proof is a later change, not a widening of this one.
- The scoped outstanding-pending-exception cap, fired-token binding to resolved context, and separate silence-allowance counting: those are `bound-dark-window-exceptions` (#131).
- Owner-facing review, narrowing, pause/resume, and revoke intents over the scoped rule: those are `add-channel-neutral-responsibility-review`.
- Mining repeated approvals into a *proposed* responsibility is bounded here to grouping by complete context class. The evidence-class rules, the two-unique-decision threshold, and the owner-review object already exist in `responsibility-contract` and are not re-authored.
- No new connector, resolver, or executor. No widening of `email.create_draft` authority and no authorization of `email.send`.
