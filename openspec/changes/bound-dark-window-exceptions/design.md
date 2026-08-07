# Design: Bound dark-window exceptions

## The amplification is a missing cap, not a weak fingerprint

It is tempting to read the invariant — "varying payload, target, grant, or owner-surface identifiers cannot turn exhausted quota into an unbounded silence queue" — as an instruction to make the fingerprint coarser: key the pending row on the reviewed scope instead of on the request, so varying the payload collides into one row.

That is the wrong fix, and it would break something load-bearing. The fingerprint is not only a dedup key; it is the **token binding**. `consume_standing_rule_fired_pending` recomputes it from the request being re-dispatched and refuses unless it matches, which is what stops a waiver minted for draft A from admitting draft B. Coarsening it to the reviewed scope would make one fired token admit *any* request inside that scope — trading an unbounded queue of narrow waivers for a single unbounded waiver. Strictly worse.

So the two concerns are separated:

| Concern | Keyed on | Enforced by |
| --- | --- | --- |
| How many exceptions may be outstanding | `(rule_id, rule_version)` — the reviewed responsibility | the cap, counted in the scheduling transaction |
| What one fired exception may admit | the exact request digests | the existing fingerprint predicate, unchanged |
| Whether a fired exception may still fire | the reviewed scope + compatibility epoch | new revalidation at consume time |

The cap alone closes the amplification: at a cap of one, the hundredth varied payload finds the slot taken and is refused. The fingerprint keeps doing its narrower job.

## The cap is counted where the row is written

`schedule_standing_rule_dark_window` already runs in `TransactionBehavior::Immediate`. The cap is evaluated inside that same transaction, in this order:

```
BEGIN IMMEDIATE
  1. dedup: does a row already exist for (rule_id, rule_version, fingerprint)?
       yes -> existing idempotent behaviour, consume no slot, return it
  2. count nonterminal rows for (rule_id, rule_version)
       resolved_at IS NULL  -- open
       (a resolved row, allowed/denied/stale, is not outstanding)
  3. count >= max_pending_exceptions -> SUPPRESS: no row, no timer,
       no *scheduled* evidence (a refusal audit IS written)
  4. otherwise insert the row, the timer, and the scheduled audit
COMMIT
```

Two properties follow from doing it here rather than in a pre-check:

- **No orphan state on refusal.** Nothing is inserted before the count, so a suppressed request cannot leave a row whose timer was never created or a timer whose row was rolled back. Counting before inserting is deliberate; the alternative — insert, count, roll back — would make the refusal path depend on rollback correctness for its safety.
- **Concurrency needs no new machinery.** `BEGIN IMMEDIATE` takes the write lock at statement one, so two racing callers serialize across processes and across separate connections; the second observes the first's committed row in its count. This is the same TOCTOU closure D-050 established for quota and rate, applied to the exception slot. Note the in-process `Store` additionally holds one `Mutex<Connection>`, so a single-process test cannot distinguish `Immediate` from `Deferred` — `concurrent_requests_cannot_cross_the_final_slot` proves the invariant, not the mechanism.

Deduplication is checked *before* the count so an idempotent repeat of an already-scheduled request never fails at the cap. A repeat is not a new exception.

### Nonterminal, not "unresolved-and-unfired"

The count uses `resolved_at IS NULL`. A row that the owner allowed, the owner denied, the timer fired, or a lifecycle change staled is resolved and no longer occupies a slot. This is what makes the cap a bound on *outstanding* exceptions rather than a lifetime quota: once the owner has answered — or the default has been applied and audited — the responsibility may schedule its next exception. A lifetime bound would be a different policy, and a stricter one than "at most N outstanding," but it is not what "outstanding pending exceptions" means and it would silently retire a rule the owner never retired.

## Suppression is invisible to the caller

A suppressed dark window returns the ordinary `ApprovalRequired` decision with no budget movement and no `dark_window_scheduled` flag. The caller cannot distinguish it from "this rule has no dark window." That is deliberate: `StandingRuleBudgetInfo` is already withheld on anything but an authorized Allow (AD-013/AD-106) precisely so a denial does not leak capacity state, and a distinguishable "suppressed at cap" signal would re-introduce that leak in a new field — it would tell a worker exactly when the exception slot frees up.

The gate outcome distinguishes the two internally, because the kernel must not send an owner notification for a schedule that did not happen; only the response is uniform.

## Notification pressure is bounded by the cap, not by a new surface

The issue asks for bounded owner notifications so a looping worker cannot flood the owner channel with one card per payload. That flood is a direct consequence of unbounded scheduling: the pending-button card is sent only when `dark_window_scheduled` is true. Once at most `max_pending_exceptions` schedules can exist per rule version, at most that many cards can be sent, and suppressed requests send nothing.

An aggregate digest surface is therefore not built here. It would add a rendering mechanism without removing a risk, and the honest bound is the one the owner reviewed.

## Binding the exception to the reviewed context

When a scoped rule schedules an exception, the pending row records the resolved context's `reviewed_scope_digest` and `compatibility_digest`. Both are nullable, because a rule with no scope binding can still have a dark window and has no context to bind — it is simply ineligible for scoped admission after #128.

At consume time the existing predicate set (resolution `allowed`, `resolved_at` set, `token_consumed_at` NULL, matching fingerprint, rule still active at that version and unexpired) gains: the stored digests must equal the freshly resolved context's. This closes a real window that #128 left. #128 made drift stop a rule from *matching* at consultation, but a token minted before the drift was still consumable afterwards, because consuming checks the rule row, not the context. Now a drifted context cannot spend a pre-drift waiver.

The comparison reuses the digests rather than re-running `ReviewedActionScope::compare`, for the same reason #128 uses them as a pre-filter: the canonical comparison already ran when the rule was selected, and what a fired token needs is the cheap equality question "is this the same reviewed instance the exception was minted for?" A corrupt persisted binding is still caught at selection by the canonical comparison, so this adds no second comparison implementation.

## Exception accounting is not quota accounting

A fired `Allow` is an explicit exception. Three consequences:

- **Distinct audit class.** `standing_rule.exception_fired` records the fire, separately from ordinary admission evidence, so an auditor can total silence-admitted effects without unpicking the quota ledger.
- **Never pooled.** The allowance is counted per `(rule_id, rule_version)`, exactly like the cap. Two responsibilities sharing an action do not share exceptions, matching the no-budget-pooling invariant #128 established.
- **Silence does not keep a rule alive.** A fired exception does **not** refresh `last_used_at`. Lapse-after-unused exists so a responsibility the owner has stopped exercising retires itself; if owner silence refreshed that clock, a rule could be kept alive indefinitely by exactly the signal that should retire it. The issue left this "unless explicitly decided" — it is decided here, and it is the fail-closed direction. The drift trigger is still evaluated on a fired exception, because a rule saturating through silence is precisely one the owner should re-review.

## Lifecycle staleness

Five transitions must not leave a fireable exception behind:

| Transition | Site |
| --- | --- |
| revoke | `revoke_standing_rule` |
| expiry / lapse | `active_standing_rule_for_action`, `consult_and_reserve_standing_rule`, `consult_and_reserve_scoped_rule` |
| drift → `needs_review` | `note_standing_rule_use_in_tx` |
| version bump | `activate_standing_rule_in_tx` (prior-version rows) |
| stored `Allow` becomes ineligible | `sweep_ineligible_dark_window_allow_rules` (at open) |

Each marks every unresolved pending row for the affected `(rule_id, rule_version)` `stale` with `resolved_at` set, in the same transaction as the transition — a staleness write that could be lost relative to its trigger would be worse than none, because it would look like protection. `claim_standing_rule_dark_window` already treats `denied`/`stale` as terminal and grants no authority, so the claim path needs no new branch; what it needed was for something to actually write `stale`, which nothing did.

One further change is caught differently and deliberately, and is **not** a staleness write: a **compatibility-epoch or reviewed-scope change** has no lifecycle transition to hang a staleness write on — nothing tells the kernel "this context drifted", it is simply resolved differently next time. It is therefore refused at consume time instead, by the digest revalidation in `standing_rules_fired_token.rs`. The consequence is that a drifted exception keeps occupying its cap slot until its timer fires and resolves it. That is acceptable: the slot is bounded, the row is unfireable from the moment the context moves, and the alternative — sweeping every pending row on every resolution — would re-resolve contexts on a schedule the owner never asked for.

Because stale rows are resolved, they also stop occupying a cap slot. That is correct: a revoked rule's exceptions are not outstanding, and if the owner re-activates a new version, its slots are its own.

## Currently unreachable in production

One part of this change has no production path today, and saying so here rather than only in a review thread is the point of this section.

**Scoped consultation schedules no dark window at all.** `consult_and_reserve_scoped_rule` contains no timer-scheduling code; only the action-keyed `consult_standing_rule_gate` does. So no scoped rule can mint a pending exception, the action-keyed path passes `None`/`None` for the two digests deliberately, and the `reviewed_scope_digest`/`compatibility_digest` columns on `standing_rule_pending_actions` are **always NULL** in a running system today.

Consequently the requirement "A drifted context cannot spend a pre-drift waiver" is proven at store level only, by `a_drifted_context_cannot_spend_a_pre_drift_waiver`, which schedules with explicit digests and then presents mismatched ones. There is no end-to-end test because there is no end-to-end path.

It lands ahead of its wiring on purpose. A dark window for `email.create_draft` is forbidden by its own descriptor (`DarkWindowPolicy::Prohibited`) and by D-146, and no action is on the `Allow` eligibility allowlist, so the production path *should* stay unreachable until an explicit catalog decision opens it. Landing the binding now means that decision cannot also be the moment the binding is designed: whoever adds the first eligible action inherits a fired token that already revalidates its reviewed context.

## Communication `Allow` gains enforcing code

`responsibility-contract` already requires that reusable delegation reject a dark-window `Allow` default for communication and connector-write effects. Nothing enforces it: `StandingRuleManifest::validate` never looks at the pairing, and a manifest declaring `dark_window.default = Allow` on `email.send` activates today.

Enforcement goes where the required-dimension check went in #128 — at activation, the first point with the catalog descriptor in scope — via the same `Store::reject_*` shape, before the transaction opens so the refusal audit survives. Eligibility is an explicit **allowlist**, empty today: an action is `Allow`-eligible only if it is named, and an id the catalog has never heard of is ineligible too.

An allowlist rather than a classifier, because the first attempt at this was a classifier — eligible *unless* the action could be shown to be counterparty-facing, egress-classed, or descriptor-backed — and it refused five ids out of roughly fifty while reading as strict. `coolify.deploy`, `filesystem.host_write`, `secret.rotate` and every uncatalogued id stayed eligible. A rule that permits whatever it cannot classify is not fail-closed, whatever its prose says.

Activation is not retroactive, so a startup sweep converges stored state: any active rule whose stored default is `Allow` for an ineligible action moves to `needs_review` with its exceptions staled, in one transaction. Without it the prohibition would be true only of new activations.

This does not weaken the existing prohibition; it is the first code that makes it true.

## Rejected alternatives

| Option | Why rejected |
| --- | --- |
| Key the pending row on the reviewed scope instead of the request | Destroys the token binding: one fired waiver would admit any request in scope. Worse than the bug. |
| Hard-code exactly one outstanding exception, no schema field | Correct default, but invisible to the owner. The review object must state the bound, and a validated field with default 1 says it without inviting a large one. |
| Insert first, count, roll back at the cap | Makes the refusal path's safety depend on rollback. Counting first cannot leave an orphan row or timer. |
| Count a lifetime total of fired exceptions rather than outstanding ones | A different, stricter policy that silently retires a rule the owner never retired. "Outstanding" is what the brief bounds. |
| Report "suppressed at cap" to the caller | Re-introduces the capacity-state leak AD-013/AD-106 withholds on denials, and tells a worker when the slot frees. |
| Build an aggregate digest notification surface | The cap already bounds cards structurally; a new surface would add rendering without removing risk. |
| Let a fired exception refresh `last_used_at` | Owner silence would keep alive exactly the rule that lapse-after-unused exists to retire. |
| Re-run `ReviewedActionScope::compare` at consume time | A second comparison site for a question the stored digests already answer; the canonical comparison already ran at selection. |

## Note on D-107's wording

D-107 says an ambiguous dispatch outcome "finalize[s] the reservation and fence[s] the fired token." D-157 (#128) settled the mechanism differently: the reservation is *retained* as `reserved` rather than finalized to `committed`, because only a `reserved` row can later be released if reconciliation establishes the write never landed. The intent D-107 states — budget consumed, retry fenced — is unchanged, and a `reserved` row counts against every headroom query. This change does not revisit that; the divergence is noted here so a reader of D-107 is not misled, and D-157 governs the mechanism.
