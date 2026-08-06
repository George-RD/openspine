# Design: Unify approved and delegated effect execution

## Existing descriptor axis is the readiness contract

Readiness uses D-146's existing `ActionImplementationDescriptor`, not a parallel execution-class enum. The descriptor is action-scoped through its `action_id` and declares the concrete implementation id, connector kind, executor id/version, and resolver id/version. The canonical `email.create_draft` entry is:

- implementation id `gmail.draft.v1`, version `1`;
- connector kind `gmail`;
- executor id `gmail.create_draft`, version `1`;
- resolver id `gmail.thread_recipient`, version `1`.

`ActionCatalog` already owns implementation descriptors keyed by implementation id. The action readiness query scans those values for the requested `ActionId`, then checks the executor id in the kernel-owned `EffectExecutorRegistry`. Thus `AppState::is_execution_backed` is true only for the conjunction of an action-keyed descriptor and a registered function pointer. A descriptor documents an intended implementation; it never asserts that a runnable executor is present.

The registry uses the existing handler-registry shape: a static registration table keyed by `&'static str`, a `HashMap` owned by `AppState`, and lookup that returns the copied function pointer. The first registration is `gmail.create_draft`; no connector or resolver is selected by an approval source.

## `EffectPathClass` cannot classify an action id

`EffectPathClass` describes a named path (`EffectPath { name, classification }`). The catalog stores those paths as a flat list without an action key. It can characterize that a path is `post-gate-approved-effect` or `gated-shell`, but it cannot answer whether an arbitrary `ActionId` has a concrete executor. Implementation readiness therefore stays on `ActionImplementationDescriptor` plus registry membership rather than deriving a new action classification from effect-path labels.

## Deny-by-default dispatch and the closed stub allowlist

The old miss arm treated every handler-registry miss as a harmless stub. A positive predicate such as "has a delegation descriptor" or "is counterparty-facing" is too narrow: the catalog contains 50 reachable action ids while the production handler registry contains only 15 registered handlers, leaving many unregistered writes, mutations, connector operations, and external effects that such a predicate would incorrectly allow to report success.

The replacement is an explicit `ActionCatalog::non_effect_stub_actions` set. It contains exactly these seven catalogued READ ids, each with no kernel-side implementation and no dedicated production route:

- `memory.read:owner_preferences_limited`
- `memory.read:writing_preferences_scoped`
- `email.read_inbox`
- `email.read_thread:unselected`
- `email.read_attachment`
- `filesystem.host_read`
- `vault.secret_read`

Only a catalogued READ action meeting that rule may return the old stub response. `artifact.write:task_scratch`, `model.generate:approved_provider`, and `briefcase.topup` remain excluded because they are writes or have dedicated routes that perform real work. Every other registry miss returns `DispatchError::NoExecutor(ActionId)`, including `email.send`, `coolify.deploy`, and unknown action ids. `is_non_effect_stub` is false for unknown ids, so an untrusted or misspelled id cannot gain stub eligibility.

`NoExecutor` is classified as a proven pre-effect failure. Existing dispatch cleanup therefore cancels consult and fired standing-rule reservations and re-arms a fired one-use token only after successful cancellation. The generic shell dispatcher does not attempt the Gmail executor because it has only `Option<&serde_json::Value>` and cannot construct the protected digest-bound request needed for kernel re-derivation.

## One executor with a truthful outcome

`create_approved_draft` remains the implementation of the existing approval flow, but changes from `anyhow::Result<()>` to `anyhow::Result<EffectOutcome>`. Its existing checks and evidence stay in place:

1. Load the protected payload and compare its kernel-derived digest with the approved digest.
2. Resolve the Gmail connector and fetch the live thread.
3. Re-derive the newest non-owner recipient and compare the target digest.
4. Take the Gmail write permit, then insert the pending-draft-write fence before the provider call.
5. Perform the Gmail write and resolve the pending row only on a confirmed provider result.
6. Preserve the existing audit, notification, failure-breaker, and idempotency evidence.

The outcome labels are deliberately narrow:

- `RefusedPreEffect`: a known refusal occurs before a provider write can be attempted (payload/target mutation, missing connector, refused thread/recipient derivation, or rejected write admission).
- `DeliveryUnknown`: the provider write outcome is unknown; the pending row remains open for reconciliation.
- `FailedAfterAttempt`: a write attempt happened and the provider failure is not delivery-unknown; the existing failure audit/batch evidence remains authoritative.
- `Executed`: the provider confirms the write and all existing post-write evidence completes.

Only `Executed` and `DeliveryUnknown` permit that an external write may have reached the provider. The two delivered callers do not own a standing-rule reservation, so they do not reinterpret the outcome into a budget decision. The headless caller appends `headless.approved_dispatched` only for `Executed`; all other outcomes return after the executor's truthful audit path.

## Admission-source convergence

The per-instance digest-bound approval handler calls the executor implementation directly with the already persisted `ActionRequest`. The headless approved lane re-gates the same request, checks the descriptor and registry, and then invokes the same executor before generic dispatch. This fixes the second live route that previously received the generic stub: it routes **any approved digest-bound `email.create_draft` request** through the executor. A request minted by today's `run_headless_hook` has `target_ref: None` and the raw webhook body as payload, so target re-derivation returns `RefusedPreEffect` and no draft is created; the convergence path uses a reviewed target and payload.

The future scope-matched standing-rule admission in #128 will also address `gmail.create_draft` by executor id once it can supply the kernel-resolved request and reservation context. It is not a live admission source in this change. The ordinary shell miss path fails closed instead of fabricating digest-bound context, as recorded by D-153.

## Audit, idempotency, and verification surface

The executor owns the draft's existing private-reference audit and pending-write evidence. It emits `draft.created` only for a confirmed creation, keeps delivery-unknown pending for reconciliation, and reports refusal or post-attempt failure with their existing audit kinds. The headless dispatch audit is an additional convergence marker only after `Executed`, so it cannot turn a refusal or uncertain delivery into a success claim.

Tests cover the seven-id stub boundary, missing-executor cancellation, descriptor-plus-registry readiness, both admission sources, payload/target mutation, delivery-unknown pending state, successful execution, post-attempt failure, and truthful audit counts. The outcome evidence is `payload_mutated_since_approval_is_denied_and_creates_no_draft`, `target_mutated_since_approval_is_refused_without_a_draft`, `draft_write_timeout_is_delivery_unknown_and_leaves_pending_row`, `successful_draft_write_is_executed_and_resolves_pending_row`, and `definite_write_failure_is_failed_after_attempt_and_resolves_the_fence`; write-admission ordering is covered by `rate_limited_write_admission_is_refused_without_a_fence_row` and `unavailable_gmail_connector_refuses_before_any_fence_row`.

## Out of Scope

- Resolving a webhook envelope into a draft payload plus a reviewed thread target is not part of this change. `webhook_minted_headless_draft_refuses_before_any_write` enforces the production boundary: the unresolved webhook request refuses before any write. The resolved-context work belongs with #128.
