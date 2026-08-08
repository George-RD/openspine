# Design: Add channel-neutral responsibility review and lifecycle controls

## The review object is persisted whole, content-addressed, and digest-bound

`OwnerReviewRequest` (`crates/openspine-schemas/src/owner_review.rs`) already exists with a private `binding_digest` and `binding_is_valid()`. This change persists it as the single security-relevant record every owner surface renders.

We serialize the review plaintext and store it through `ArtifactStore::put` (`crates/openspine-kernel/src/artifact_store_io.rs:8`), keeping the returned `ArtifactRef`. Retrieval reuses `ArtifactStore::get` (`artifact_store_io.rs:73`), which re-verifies the SHA-256 digest at read time, and we additionally call `OwnerReviewRequest::binding_is_valid()` before any decision may be recorded. A `review` store row keys the review by id, its `ArtifactRef`, its `OwnerReviewState`, the bound owner `Principal` id, and `expires_at`, with the `append_audit_conn` write inside the same transaction as the state transition. This is the same content-addressing + read-time re-verification idiom the artifact store already gives every other security-relevant payload (D-055.4), applied to the review object.

The stored review and the rendered text cannot diverge because the binding digest covers the whole structured object (D-045 WYSIWYS parity). A channel adapter can only present the object and submit an intent; it cannot construct a different scope or limits.

## One review object, one binding digest, two adapters

The renderer abstraction is a kernel-owned trait over two implementations — Telegram (verified via `VerifiedOwnerContext`, `telegram.rs`) and the local terminal (verified via the `local_cli_owner` envelope: `Source::Cli` + `VerificationMethod::LocalCliAuth`, `pipeline/lanes.rs:199-205`). Both adapters render the same `OwnerReviewRequest` and submit a principal-bound `DecisionIntent` against the same binding digest. Neither adapter may add scope, decisions, or lifecycle logic.

This is explicitly the D-146 design: the contract contains no chat id, callback id, Telegram method, or terminal command field. The adapter is a presentation/input seam, never an authority or lifecycle owner. The kernel is the only thing that transitions state.

## Decisions are principal-bound, digest-bound, and kernel-verified

Every decision intent is authenticated to the owner principal (Telegram via `VerifiedOwnerContext`; terminal via `local_cli_owner`), bound to the stored review's binding digest, routed through the kernel's existing decision path, and audit-recorded.

`DecisionIntent` is defined as a **total mapping onto the union of the two existing digest-bound decision sets** in `crates/openspine-schemas/src/owner_review.rs`:

| `DecisionIntent` | Source set | Membership gate |
| --- | --- | --- |
| `Approve` | `OwnerReviewDecision` | must be in `available_decisions` |
| `Reject` | `OwnerReviewDecision` | must be in `available_decisions` |
| `Narrow` | `OwnerReviewDecision` | must be in `available_decisions` |
| `Edit` | `OwnerReviewDecision` | must be in `available_decisions` |
| `Pause` | `ResponsibilityLifecycleControl` | must be in `lifecycle_controls` |
| `Resume` | `ResponsibilityLifecycleControl` | must be in `lifecycle_controls` |
| `Expire` | `ResponsibilityLifecycleControl` | must be in `lifecycle_controls` |
| `Revoke` | `ResponsibilityLifecycleControl` | must be in `lifecycle_controls` |
| `Inspect` | read-only intent, in neither set | **exempt** from the membership check |

`Edit` and `Expire` are not dropped: they are legal intents drawn from the existing enums and gated by the same membership rule as their siblings. `Inspect` is a read-only intent that causes **no state transition**; it is exempt from the membership check precisely because it is read-only, and a scenario asserts it never mutates state. Every other intent is gated by the membership rule, so a surface cannot offer a decision that was not part of what the owner reviewed.

**D-011 seam (one sentence):** review-Approve records the decision on the review row — the review row is the sole disposition of the owner's decision; it does **not** mint an `ApprovalRecord`, and the approved effect still crosses `gate()` as an ordinary digest-bound `ActionRequest` under a fresh task grant (D-007), so D-011's digest-bound-at-effect guarantee is preserved at the effect boundary rather than by reusing `ApprovalRecord` for the review decision itself.

## A dedicated `OwnerReviewState`, not a reuse of `Lifecycle`

The review object gets its own `OwnerReviewState` enum — `Pending / Approved / Rejected / Narrowed / Revoked / Expired` — with a `can_transition` validator, mirroring the `Lifecycle` enum-plus-validator idiom (`crates/openspine-schemas/src/artifact.rs:60`). We deliberately do **not** reuse the artifact `Lifecycle` enum: `Lifecycle` models artifact propose→approve→activate and has no owner-only paused semantics.

`Paused` and `Resumed` are **not** review states. Pause and resume are standing-rule runtime statuses (see the next section), not review-object states, so a rule paused and resumed twice has no representable review state — correctly, because the review object itself is unchanged by a pause. The review state machine and the standing-rule status machine are two separate axes, each with its own typed enum and its own `CHECK`/validator, so neither is overloaded with the other's semantics. `needs_review` is likewise a standing-rule/system status, not a review state; the review object distinguishes owner-controlled decisions from system-triggered re-review by the *provenance* of the transition, not by a `needs_review` review state.

Which state machine each intent mutates is explicit:

| Intent | Mutates |
| --- | --- |
| `Approve`, `Reject`, `Narrow`, `Edit`, `Expire` | the review object's `OwnerReviewState` |
| `Pause`, `Resume`, `Revoke` | the standing-rule runtime status (`active`/`paused`/`revoked`) |
| `Inspect` | neither (read-only) |

## Narrow produces a new immutable digest

Narrow is a distinct review-producing decision: the owner narrows one or more reviewed-scope dimensions and the kernel constructs a **new** `OwnerReviewRequest` with a new `ReviewedActionScope` and a new binding digest, persisted as its own content-addressed record. The new object reuses the narrowed dimension values (per-dimension comparability, exactly what `define-responsibility-contract`'s scope-binding already stores and what #128's `reviewed_scope_digest` makes matchable), but the digest differs because the scope differs. The original review object remains immutable; a narrowed decision can never be replayed as if it approved the broader original.

Because the narrowed scope is a valid `ReviewedActionScope` with its own context-class digest, it remains an enforceable, matchable scope for #128's scoped admission — narrowing never degrades a reviewed scope into an unenforceable one.

## Owner surface reference, not a naked chat integer

The generic review, decision, pending-action, notification, and receipt code MUST NOT accept a naked `bound_chat_id: i64`. We introduce a kernel-owned `OwnerSurfaceRef` / reply-target contract that can represent at least: verified Telegram private owner chat, authenticated local terminal/device session, future verified web/mobile owner session, and optional channel-thread binding (AD-148). The surface reference is typed, principal-bound, minted only by the adapter that authenticated the surface, and separated from connector-specific rendering ids.

It is **not** MAC-covered where it crosses a grant, and this change does not claim otherwise. `TaskGrant::verify_mac` covers `grant_json`; the bound surface lives in the sibling `task_grants.owner_surface_json` column, so direct database write access can repoint a grant's reply surface without invalidating the grant's MAC. That is the same trust boundary the grant's other sibling columns already sit inside, and it is still weaker than sealing: the honest options are to move the surface into the sealed grant or to extend the MAC over the column, both of which are larger changes than this one and are left open (`tasks.md` 5.2, deliberately unticked).

- Telegram ids may remain in adapter storage (`telegram.rs`), but generic code does not consume a raw integer.
- Terminal review must **not** fake a Telegram chat id to reuse the lifecycle; the terminal surface has its own `OwnerSurfaceRef`.
- Migration/compatibility tests prove existing Telegram grants and pending rows remain valid or fail closed, and that the terminal lane never synthesizes a Telegram id.

This is the #126 "name to settle" seam: the ref is introduced here because the review/lifecycle code is the first generic consumer that must be channel-neutral. Connector-specific rendering ids stay in the adapters.

### Where the boundary is drawn, and how to check it

"Generic seam" is not a matter of taste, so here is the test a future reader
can apply. A seam is **generic** if the value it holds is a *persisted grant
binding* — the answer to "which authenticated owner surface does this grant
belong to?" — and the code holding it is not itself a channel adapter. Every
such seam was cut over to `OwnerSurfaceRef`: `api::authenticate` and the whole
`api/actions.rs` dispatch chain (including the `ActionHandler`,
`EffectExecutor`, and `PostApprovalHandler` function-pointer types),
`pipeline/approval.rs` and `pipeline/plan_approval.rs` (whose channel-binding
check is now a whole-surface comparison, so it also catches a *cross-channel*
replay an integer comparison structurally could not), `store/standing_rules_
pending.rs` and the standing-rule timer, worker dispatch and the worker-result
relay's dispatch call, `workflow.rs`'s `GatedStepDigest`, `EventInputs`,
`AppState::lock_conversation`, and the `notify_owner_*` backbone. The
`task_grants.bound_chat_id` and `standing_rule_pending_actions.bound_chat_id`
columns are dropped by a versioned migration.

A seam is **adapter-internal** if the value is a Telegram address held by code
that already hard-codes the Telegram connector, its breaker, and its retry
row, and that is reachable only after `telegram::telegram_chat_id` has
resolved a surface. Four of these deliberately keep an `i64` and MUST NOT be
converted on the strength of the cutover:

| Site | Why it is adapter-internal |
| --- | --- |
| `store/failure_surfacing_types.rs`, `store/failure_surfacing.rs` — `notify_dead_letters.chat_id` | The dead-letter row exists to *retry a Telegram send*. It is written only by `notify_owner_with_digest` after that function has resolved the surface, and read only by `failure_surfacing/retry_worker.rs`, which re-mints a Telegram surface from it. A channel-neutral column would have nothing to retry against. |
| `store/worker_result_relay.rs` | Same row, same retry path, reached through the same resolution. |
| `test_support.rs` | Fixture constructors (`telegram_surface`, `owner_surface_for`) that mint surfaces *from* a chat id for tests. Not a production seam. |

The boundary has two directions and both need a check, because choking only
one of them is not a boundary. **Resolve** is surface → chat id; **mint** is
chat id → surface. Generic code must be unable to do either.

**Resolve.** `telegram::telegram_chat_id` is the only way to get a chat id out
of a surface and it returns `Err` for any surface that is not a Telegram
chat, so a terminal-bound grant cannot reach a Telegram-only path even by
accident. A chat id can only *enter* a signature as a parameter or field, so:

```
grep -rn 'chat_id: i64' crates/openspine-kernel/src --include='*.rs' \
  | grep -vE '/(telegram|test_support)\.rs:' \
  | grep -vE '/store/(failure_surfacing|failure_surfacing_types|worker_result_relay)\.rs:' \
  | grep -vE '(_tests\.rs|/tests/)'
```

**must print nothing.** Anything it prints is a regression of this change.
(Locals named `chat_id` are fine and expected — each is the immediate result
of a `telegram_chat_id(...)` call inside an adapter. Test files are excluded
because fixtures legitimately construct surfaces from literal chat ids.)

**Mint.** `telegram::telegram_owner_surface` is the inverse and it performs no
verification of its own — it cannot, having neither the owner configuration
nor the update that proved the chat. It is therefore `pub(crate)` and
adapter-only, and every caller must already hold a Telegram-verified chat id:

```
grep -rn 'telegram::telegram_owner_surface(' crates/openspine-kernel/src \
  --include='*.rs' | grep -vE '(_tests\.rs|/tests/|test_support\.rs)'
```

**must print exactly three call sites**, each verified at the point of mint:
`pipeline/mod.rs`'s `AppState::telegram_owner_surface` (the configured owner
chat, for kernel-origin notifications with no inbound update);
`pipeline/mod.rs`'s owner-update lane (immediately after `verify_update` has
proven owner identity *and* a private chat); and
`failure_surfacing/retry_worker.rs` (re-addressing a dead-letter row the
Telegram notifier itself wrote from an already-verified surface). Everything
else reaches a surface through `AppState::telegram_owner_surface()`.

`notify_owner_surface_required` is the seam that routes a grant-bound surface
to whichever adapter actually owns it, so a terminal-bound grant's escalation
goes to the terminal instead of being fired at a Telegram chat.

What would move a site across the boundary: a second delivery channel gaining
its own dead-letter retry. At that point `notify_dead_letters` stops being
Telegram-adapter storage and its `chat_id` becomes a generic reply target that
must be an `OwnerSurfaceRef`. Until then, converting it would add a
channel-neutral type over a channel-specific fact and buy nothing.

## Paused state is a typed standing-rule status, not a review state

The standing rule store writes free-form `status TEXT` (`store/standing_rules.rs:105-110`) with no constraint. We add `paused` to the allowed status set and add a `CHECK(status IN (...))` constraint. We audit every live-consultation site that filters `status = 'active'`:

- `active_standing_rule_for_action` (`standing_rules.rs:304`)
- `standing_rule_is_current` (`standing_rules.rs:434`)
- `reserve_standing_rule_budget` / `standing_rules_budget.rs:49,115,315`
- the currency re-check in `finalize_standing_rule_reservation`
- the `EXISTS` guard in `standing_rules_fired_token.rs:89-92`

so a `paused` rule is absent from live consultation. A pause therefore immediately restores ordinary approval behavior — the rule leaves live consultation, no scoped admission can match it, and no pending dark-window timer can fire against it.

`pause_standing_rule` and `resume_standing_rule` sit alongside `revoke_standing_rule` (`standing_rules.rs:277`). Each writes a distinct audit event (`standing_rule.paused`, `standing_rule.resumed`) mirroring `standing_rule.revoked`. Revoke stays immediate and idempotent (touch only rows not already `revoked`).

Supersession behavior: `activate_standing_rule_in_tx` (`standing_rules.rs:180`) currently revokes every other `active` rule for the same action. When a new version activates over a **paused** rule, the paused rule is also superseded — it must not silently reappear on a later resume, because the owner reviewed and paused an older version that a newer version has replaced. We document and enforce that a superseded paused rule transitions to `revoked` so a resume of a stale paused rule fails the version-staleness re-check.

## Resume revalidates compatibility, modeled on artifact.reconfirm

Resume is the highest-scrutiny lifecycle transition, so it reuses the `artifact.reconfirm` ceremony shape (`pipeline/artifact_reconfirmation.rs`). Before returning a paused rule to `active`, the kernel MUST:

1. Re-verify the reviewed bytes and digest (the stored `OwnerReviewRequest` binding).
2. Re-check the rule is still the exact version that was paused (not superseded).
3. Revalidate policy, descriptor, executor readiness (`AppState::is_execution_backed`, `pipeline/mod.rs:183`), connector/account health (reusing `ConnectorRegistry::breaker_state`, `connectors.rs:262` — no connector-specific branches in generic resume code), and the reviewed scope.

The reviewed-scope revalidation is a first-class check because #128 is making standing rules scope-bound and archives first. Resume MUST compare the rule's bound reviewed scope against the freshly resolved context through the canonical `ReviewedActionScope::compare` (`crates/openspine-schemas/src/reviewed_scope.rs`), and a corrupt persisted scope binding MUST surface as the existing invalid-scope outcome rather than matching on either half. A rule whose reviewed scope no longer matches the resolved context is drifted and MUST NOT resume.

An expired or drifted rule is refused and requires a new reviewed version instead, with a distinct audit event per rejection reason. A concurrent resume tap is a safe no-op via the `committed: bool` "race already handled, say nothing" idiom (as in `artifact_reconfirmation.rs`).

## Proposal copy cannot outrun evidence, scope, or readiness

Rendered owner-facing copy is derived from stored provenance (`ProposalProvenance`), names only the reviewed scope digest, and never claims an observed pattern unless `DelegationEvidenceKind::RepeatedApprovals` supports it (already enforced by `define-responsibility-contract`). It never claims the reusable effect path is ready unless `is_execution_backed` is true (from #127). Review receipts are truthful to what the decision actually committed and never restate a false success. A review decision commits a *lifecycle/authority* outcome — approving activates the derived standing rule — while the effect is a separate, separately gated `ActionRequest` under a fresh task grant (D-007). A review receipt therefore names the review, the intent and the binding digest, and never claims an external effect executed or was delivered. The `EffectOutcome` truthfulness contract from #127 governs the receipt for that effect, on the effect path, and is unchanged here; this change deliberately does not re-derive a second one.

## Authority, containment, audit, failure modes

- **Authority.** A review object is not live authority (D-007). `Approve`/`Reject`/`Narrow`/`Edit`/`Expire` mutate the review object's `OwnerReviewState`; `Pause`/`Resume`/`Revoke` mutate the standing-rule runtime status; `Inspect` mutates neither. None of them mint a grant or widen what `gate()` permits. Every admitted task still crosses `gate()`.
- **Containment.** Every value in the reviewed scope is kernel-resolved (D-146/AD-036). A surface cannot widen scope; `Narrow` only removes or tightens dimensions within the already-reviewed object.
- **Audit.** Every decision intent and every lifecycle transition writes a durable, distinct audit event. The review row, its artifact ref, its state, the bound principal, and `expires_at` are all audited in the same transaction as the transition.
- **Failure modes.** A digest mismatch fails closed (binding_is_valid false → no decision eligible). An expired review is not eligible. A paused/superseded/expired/drifted rule refuses resume with a distinct reason. An opaque or truncated rendering refuses persistence of an approvable review.
- **Prompt injection.** Nothing in the reviewed scope or the decision intent originates in model output or connector content. Injected text can influence an *intent* the owner might submit, but it cannot fabricate the binding digest, the reviewed scope, or the principal binding — those are kernel-authored and digest-covered.

## Rejected alternatives

| Option | Why rejected |
| --- | --- |
| Reuse the artifact `Lifecycle` enum for review state | `Lifecycle` models propose→approve→activate and has no owner-only `paused` semantics. Overloading it couples two unrelated lifecycle axes and would force a fake "activate" for an owner pause. A dedicated `OwnerReviewState` keeps decision availability explicit (CodeRabbit design choice 1). |
| Generalize `resolve()` into a trait of channel proofs, one per channel | A large identity-layer refactor for a change that only needs to bind the existing owner `Principal`. Reusing the existing per-channel verification seams (`VerifiedOwnerContext`, `local_cli_owner`) and binding the resolved principal id onto the decision is smaller and still yields a channel-neutral principal_id (CodeRabbit design choice 2). |
| Require the full propose → evaluate → approve → activate ceremony for every resume | Over-forces owners through a full re-approval for a still-compatible, still-current version. The `artifact.reconfirm` shape already implements exactly the resume-with-revalidation pattern and refuses expired/drifted rules (CodeRabbit design choice 3). |
| Store the review state by overloading the standing-rule `status TEXT` column with review semantics | Review state and standing-rule runtime status are different axes; a review's `OwnerReviewState` must not be conflated with a rule's `paused`/`active`/`revoked` status. Separate typed enums and separate rows keep each honest. |
| Keep passing `bound_chat_id: i64` through generic review/decision code | The #129 comment names this exact seam: it is Telegram-shaped even outside the renderer. The `OwnerSurfaceRef` is the fix; naked integers stay only inside adapter storage. |
| Have terminal review synthesize a Telegram chat id to reuse the lifecycle | Fabricates an identity the terminal does not hold and would let one surface impersonate another. The terminal lane has its own `OwnerSurfaceRef` and must not fake a Telegram id (migration/compat test). |
| Let Narrow write back onto the original review object | A digest-bound review is immutable (D-011/D-045). Mutating it would invalidate the very binding the owner approved. Narrow must produce a new immutable digest. |
| Invent a second approval mechanism for Approve/Narrow | Duplicates the D-011 WYSIWYS guarantee and risks divergent semantics. The review row is the sole disposition of the owner's decision; the approved effect still crosses `gate()` under a fresh task grant (D-007), so D-011's digest-bound-at-effect guarantee is preserved at the effect boundary without a parallel `ApprovalRecord` path. |
