# Bound dark-window exceptions

## Dependencies

- `define-responsibility-contract` (archived, **HARD**): supplies `ResolvedActionContext`, `ReviewedActionScope`, and the D-146 rule that communication and connector-write effects MUST reject a dark-window `Allow` default. The pre-seeded `responsibility-contract` spec already carries that prohibition as a requirement.
- `mine-and-match-reusable-authority-by-scope` (#128, archived, **HARD**): supplies the reviewed scope binding, `reviewed_scope_digest`, the compatibility epoch, and the scoped consultation this change caps. Without it there is no "reviewed scoped rule" to bound exceptions per.
- Canonical decisions: D-146, D-107, AD-012 as narrowed by D-146, plus D-155..D-158 from #128.

This change affects **OpenSpine core**, not Lyra product surfaces. It affects **runtime authority**: it bounds the one path where owner *silence*, rather than owner approval, admits an effect. It adds no connector access, no external communication, and no system-operations authority; it only removes authority that is currently reachable.

## Problem/Context

A dark window is a timer-boxed conditional grant: "if you do not respond, apply the pre-agreed default." Today the kernel persists one pending action and one timer per unique `(rule_id, rule_version, request_fingerprint)`, and the fingerprint is

```
digest_of("{action}|{grant_id}|{bound_chat_id}|{payload_digest}")
```

Every component of that key is caller-varying. A worker whose quota is exhausted can vary the payload — a different subject line is enough — and receive a *fresh* pending exception, with its own timer, for each variation. There is no cap: `schedule_standing_rule_dark_window` counts nothing before inserting, and the one counting helper that exists (`pending_dark_window_count`) is dead code and not version-aware.

So the amplification is:

```
quota exhausted
→ submit N distinct payloads
→ N independent pending exceptions, N timers
→ owner is unavailable
→ N silence-based Allows can fire
```

Each request is individually idempotent and globally amplifying. That empties the meaning of a quota: the budget the owner reviewed bounds *approved* admissions, while silence is unbounded. It is a property of the landed standing-rule substrate, independent of any product proof.

Two further gaps sit alongside it:

1. **Nothing binds a pending exception to the reviewed context.** The pending row records action, grant, chat, and payload. When its token later fires, `consume_standing_rule_fired_pending` re-checks the fingerprint and that the rule is still active at that version — but not that the reviewed scope or compatibility epoch still match. #128 made drift stop a rule from matching at consultation; a token minted *before* the drift can still fire after it.
2. **The prohibition on communication `Allow` has no enforcing code.** `responsibility-contract` requires that reusable delegation reject a dark-window `Allow` default for communication and connector-write effects. Nothing in `StandingRuleManifest::validate` or in activation checks it, so a manifest declaring `dark_window.default = Allow` on `email.send` activates today. That is the same spec-claims-what-code-does-not-do defect #128 closed for the reviewed-scope binding.

## Proposed Solution

1. **Make the exception allowance reviewable data.** Add `max_pending_exceptions` to `DarkWindowConfig`, defaulting to **1** and validated to a small hard maximum. The safe default is one; an owner who wants a small budget must state it, and the review object shows it. There is never an unbounded queue keyed by distinct payloads.

2. **Enforce the cap atomically, before any row or timer exists.** Inside the same `BEGIN IMMEDIATE` transaction that schedules, count the *nonterminal* pending rows for the exact `(rule_id, rule_version)`. At the cap, refuse: no pending row, no timer, no audit of a scheduled default. Same-request deduplication is unchanged and is checked first, so an idempotent repeat never consumes a slot. `BEGIN IMMEDIATE` is the same serialization D-050 already relies on, so two concurrent requests cannot both take the final slot.

3. **A suppressed dark window is an ordinary approval, not a quieter default.** Suppression leaves the decision at `ApprovalRequired`, reserves no quota or rate, schedules no timer, and reports no pending default to the caller. The caller cannot distinguish "no dark window configured" from "dark window suppressed at cap" in its response, so suppression is not an oracle for budget state.

4. **Bind the pending exception to the reviewed context.** Persist the resolved context's `reviewed_scope_digest` and `compatibility_digest` on the pending row when a scoped rule schedules it. Note that scoped consultation schedules no dark window today, so these columns are always NULL in a running system and this binding is proven at store level only — see design.md §"Currently unreachable in production". Consuming a fired token then requires those stored values to equal the *freshly resolved* context's, in addition to every existing predicate. A token minted before drift cannot fire after it.

5. **Count silence separately from approved use.** A fired exception is an explicit exception, not hidden extra quota: it is recorded under a distinct audit class, its allowance is counted per `(rule_id, rule_version)` and never pooled across rules or scopes, and it does **not** refresh the lapse-after-unused clock. A rule that is only ever exercised by owner silence must still lapse.

6. **Stale every open exception on any lifecycle change.** Revocation, expiry, the `needs_review` transition, and a version bump mark all unresolved pending exceptions for the affected rule version `stale` before a timer can claim them. Recovery excludes stale rows and never re-opens a terminal slot, so the cap survives restart.

## Acceptance Criteria

- Many distinct over-budget requests create at most the reviewed pending allowance: with the default cap of 1, a hundred requests differing in payload, grant, and owner-surface chat id leave exactly one live pending exception.
- Concurrent requests cannot cross the final slot: racing callers produce exactly one winner and one pending row.
- Same-request scheduling stays idempotent and does not consume a second slot.
- A distinct request at the cap stays `ApprovalRequired`, creates no pending row and no timer, consumes no quota or rate, and reports no pending default.
- A fired token whose reviewed scope or compatibility epoch no longer matches the freshly resolved context grants no authority and leaves the pending row terminal.
- A fired exception is audited in its own class, consumes exactly one exception allowance, cannot be replayed, and does not refresh the rule's lapse-after-unused clock.
- Revoke, expiry, `needs_review`, and a version bump each stale every unresolved pending exception for that rule version before a timer can claim it; recovery does not re-open them.
- Activating a standing rule whose `dark_window.default` is `Allow` for a communication or connector-write action is refused, with durable evidence — the existing `responsibility-contract` prohibition gains enforcing code and is not weakened.
- `node_modules/.bin/openspec validate bound-dark-window-exceptions --strict` passes, and every delta requirement whose header already exists in a pre-seeded capability spec is carried as `## MODIFIED Requirements`.

## Invariant

**Varying payload, target, grant, or owner-surface identifiers cannot turn exhausted quota into an unbounded silence queue.** The outstanding exception allowance belongs to the reviewed responsibility, not to the request, and is bounded by a number the owner reviewed. Silence never creates more authority than that bound; a dark-window timer is not a second authority source; and no default fires against a stale, revoked, expired, drifted, or ambiguous responsibility.

## Out of Scope

- Permitting any communication or connector-write `Allow` default. This change *enforces* the existing prohibition; lifting it needs an explicit catalog decision and proposal-specific proof, which is `make-reusable-authority-evaluation-proposal-specific` (#133), not this change.
- Re-opening #128's boundaries: the scope key stays `reviewed_scope_digest`, the drift epoch stays `compatibility_digest` byte-identical, exactly-one matching and fail-closed ambiguity are unchanged, and `DeliveryUnknown` still retains its reservation as `reserved`.
- A fence reconciler that settles retained `DeliveryUnknown` reservations. That was named as follow-up work in #128 and is still follow-up work.
- Owner-facing review, narrowing, pause, and resume intents over the scoped rule, including a `pause` lifecycle state: those are `add-channel-neutral-responsibility-review`. This change stales pending exceptions on the lifecycle transitions that exist today.
- Aggregating owner notifications into a paginated digest surface. The cap already bounds notification pressure structurally — a card is sent only when a dark window is actually scheduled, and at most `max_pending_exceptions` can be outstanding per rule version — so a separate aggregation mechanism would add a surface without removing a risk.
- Dark windows for actions with no reviewed scope binding. Those rules are already ineligible for scoped admission after #128; this change bounds them by the same per-rule-version cap but does not give them context binding they cannot have.
