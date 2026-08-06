# Unify approved and delegated effect execution

## Dependencies

- `define-responsibility-contract` (archived): establishes D-146's separate action and implementation descriptors, including resolver and executor identity/version fields.
- `mine-and-match-reusable-authority-by-scope` (#128): depends on a runnable executor and will provide the scope-matched, budget-reserved standing-rule admission context.
- `ship-recurring-gmail-draft-proof` (#130): depends on the shared Gmail draft executor and its truthful delivery/audit contract.
- Canonical decisions: D-146, D-011, D-055, and D-107. The admission-source boundary is recorded as D-153 by the parent landing ceremony.

## Problem/Context

The generic approved-action dispatcher currently returns a successful `{"stub": true, ...}` response whenever its handler registry has no entry. That behavior is safe only for an explicitly declared, non-effect read action. It is a false success for catalogued writes and other effectful actions: no provider call occurs, yet the caller can record success and, for a standing-rule admission, commit a reservation against work that never ran. A positive "known-effectful" predicate would still miss many of the catalogued action ids that are not registered today.

This change also has two live approved-admission paths for `email.create_draft`. The per-instance digest-bound approval calls the existing draft handler directly. Before this change, the D-117 headless approved lane fell through to generic dispatch and received the false-success stub; it now routes **any approved digest-bound `email.create_draft` request** through the same executor, making it the second delivered admission source. Requests today's `run_headless_hook` mints from a webhook carry `target_ref: None` and the raw webhook body as payload, so they refuse pre-effect at target re-derivation and create no draft. The draft operation already performs the required kernel re-derivation and pending-write fence, but its `Result<()>` return value collapses pre-effect refusal, delivery uncertainty, post-attempt failure, and successful creation into the same apparent outcome.

The ordinary shell dispatch path cannot call the draft executor in this change: it receives only an optional JSON payload, not the digest-bound `ActionRequest` whose protected payload and target references the executor must re-derive. Scope-matched, budget-reserved standing-rule admission is therefore a future #128 caller, not a live source delivered here.

## Proposed Solution

1. Add a catalog-owned, deny-by-default non-effect stub allowlist containing exactly the seven catalogued READ actions that have no kernel implementation and no dedicated production route. A registry miss for every other action returns typed `DispatchError::NoExecutor(ActionId)` and never a successful stub; unknown ids are not stub-eligible.
2. Attach the existing D-146 implementation descriptor to `email.create_draft` and add a kernel-owned executor registry keyed by `executor_id`. `AppState::is_execution_backed` reports readiness only when both the action-keyed descriptor and its registered executor exist; a descriptor alone is not readiness.
3. Extract the Gmail draft write behind the `gmail.create_draft` executor id (`gmail.draft.v1` implementation). Preserve all payload/target digest checks, live recipient re-derivation, pending-write fencing, audit, notification, breaker, and idempotency evidence while returning a typed `EffectOutcome` that distinguishes `Executed`, `RefusedPreEffect`, `DeliveryUnknown`, and `FailedAfterAttempt`.
4. Route both live admission sources—the per-instance digest-bound approval and the headless approved lane—through this one executor. The headless lane routes any approved digest-bound request through it; when the reviewed payload and target re-derivations succeed, it can create the real draft, while webhook-minted requests intentionally refuse pre-effect at target re-derivation. The headless lane records `headless.approved_dispatched` only after `Executed`; the executor's own truthful audit evidence covers refusal, delivery-unknown, and post-attempt failure.
5. Classify `NoExecutor` as a proven pre-effect dispatch failure so existing standing-rule cleanup cancels consult and fired reservations without consuming budget, re-arming a fired one-use token only after cancellation succeeds. Reservation finalize/retain decisions for executor outcomes remain with #128, whose future caller will possess the resolved scope-bound request.

## Acceptance Criteria

- `email.create_draft` has one real kernel-owned executor used by both delivered admission sources: the per-instance digest-bound approval path and the D-117 headless approved path. The headless lane routes any approved digest-bound request through the executor; reviewed payload and target refs can produce a real draft and resolve its pending-draft row, while today's webhook-minted requests refuse pre-effect and create no draft. `headless_and_non_headless_approval_converge_on_gmail_executor`, `headless_refusal_appends_no_dispatched_audit`, and `webhook_minted_headless_draft_refuses_before_any_write` prove the convergence, refusal-audit, and webhook boundary.
- A catalogued effectful action whose handler/executor is missing cannot report success: dispatch returns `DispatchError::NoExecutor(ActionId)`, emits no successful stub value, and the existing reservation cleanup cancels any standing-rule reservation without budget consumption. The seven explicitly declared non-effect READ ids retain the existing stub shape; unknown ids never become stub-eligible.
- Readiness is truthful: `email.create_draft` is execution-backed only when its D-146 descriptor and registered `gmail.create_draft` executor are both present; descriptor-less, unknown, and unregistered actions report not ready.
- Draft execution preserves payload and target re-derivation, the pending-write fence, idempotency, and private-payload audit protections. Tests distinguish pre-effect refusal, delivery-unknown, post-attempt failure, and executed outcomes, without treating delivery-unknown as confirmed success.
- `node_modules/.bin/openspec validate unify-approved-and-delegated-effect-execution --strict` passes, and all four deltas use `MODIFIED Requirements` headers for requirements already present in their pre-seeded capability specs.

## Out of Scope

- Scope-matched, budget-reserved standing-rule admission invoking the Gmail executor is #128. The ordinary shell dispatch path receives `payload: Option<&serde_json::Value>`, not an `&ActionRequest`, so it cannot supply the digest-bound `payload_ref`, `target_ref`, and `target_digest` that the executor re-derives. That path fails closed here; this change does not create a live standing-rule admission source. The boundary is recorded as D-153 in the decision log.
- Resolving a webhook envelope into a draft payload plus a reviewed thread target is out of scope for this change; `webhook_minted_headless_draft_refuses_before_any_write` enforces that the unresolved production boundary refuses before any write. The resolved-context work belongs with #128.
- Wiring `EffectOutcome` to standing-rule reservation finalize/retain is deferred to #128. Neither delivered executor caller holds a standing-rule reservation; this change only maps a missing executor to cancellation.
- No second Gmail connector, resolver, or approval semantics is introduced. The executor remains kernel-owned and uses the existing Gmail re-derivation and provider evidence.
- This change does not widen `email.create_draft` authority, authorize `email.send`, or make any unimplemented catalogued action executable merely by cataloguing it.
