# Tasks

## 1. Catalog the closed non-effect boundary

- [x] Add `ActionCatalog::non_effect_stub_actions`, its builder, and its fail-closed query.
- [x] Declare exactly the seven catalogued READ ids that have neither a kernel implementation nor a dedicated production route.
- [x] Keep writes, mutations, dedicated routes, and unknown ids outside the allowlist; preserve the existing stub response only for declared non-effect reads.

## 2. Describe implementation readiness

- [x] Add the `email.create_draft` D-146 implementation descriptor with implementation id `gmail.draft.v1`, executor id `gmail.create_draft`, resolver id `gmail.thread_recipient`, and their versions.
- [x] Add action-keyed implementation-descriptor lookup without creating a second descriptor convention.
- [x] Add the kernel-owned `EffectExecutorRegistry`, wire it into every `AppState` construction site, and expose `AppState::is_execution_backed` as descriptor-plus-registered-executor readiness.

## 3. Extract the Gmail executor

- [x] Change `create_approved_draft` to return `anyhow::Result<EffectOutcome>` while preserving every payload/target re-derivation, connector check, pending-write fence, audit, notification, breaker, and reconciliation call.
- [x] Label pre-effect refusals, delivery-unknown, post-attempt failure, and confirmed execution with the corresponding truthful outcome.
- [x] Expose the `gmail_create_draft_executor` adapter under the fixed registry function shape and register it as `gmail.create_draft`.

## 4. Converge both approved admission sources

- [x] Keep the per-instance digest-bound approval handler on the shared executor.
- [x] Route the D-117 headless approved lane through the descriptor/registry executor before generic dispatch.
- [x] Append `headless.approved_dispatched` only for `EffectOutcome::Executed`; rely on executor-owned evidence for all other outcomes.
- [x] Do not create a live standing-rule admission source or reconstruct digest-bound context from the generic shell payload.

## 5. Fail closed at dispatch

- [x] Add typed `DispatchError::NoExecutor(ActionId)` and replace the registry-miss success stub with the seven-id allowlist check plus fail-closed error.
- [x] Update the HTTP mapping, reservation-retention, failure-class, and digest-summary matches for the new variant.
- [x] Update proposal conversion and every exhaustive production/test match without wildcard arms; retain the existing `action.dispatch_failed` audit literal.

## 6. Preserve reservation and evidence semantics

- [x] Classify `NoExecutor` as a pre-effect failure so consult and fired reservations are cancelled without budget consumption.
- [x] Re-arm a fired one-use standing-rule token only after cancellation succeeds, with no double-cancel path.
- [x] Keep delivery-unknown pending for reconciliation and prevent any refusal or uncertain delivery from emitting a dispatched-success audit.

## 7. Add behavior-focused test families

- [x] Test delegated-path missing-executor cancellation, no stub value, no `draft.created` audit, and no finalized reservation.
- [x] Test readiness for `email.create_draft`, `email.send`, and an unknown action, including distinct digest summaries for registered-but-unreachable versus unregistered executors.
- [x] Test headless and non-headless approved draft paths converge on the same re-derived target/payload and audit shape.
- [x] Test the allowlisted read stub and fail-closed behavior for `email.send`, `coolify.deploy`, `briefcase.topup`, and `artifact.write:task_scratch`.
- [x] Test payload/target mutation, delivery-unknown pending state, and successful execution outcome/audit transitions.
- [x] Run the existing effect-path, approval-reconciliation, and host filesystem gate regressions without changing their deny semantics.
- [x] Delete the `workflow.invoke:approved` and `setup.workflow.start` placeholder stub handlers so effectful ids fail closed.
- [x] Take the Gmail write permit before recording the pending-write fence and test rejected admission before any fence row.
- [x] Keep `/setup` honest after the kernel's `NoExecutor` decision: the shell surfaces the opaque `500` rather than diagnosing it as "not implemented", which would mask a genuine connector or resource outage.
- [x] Test the webhook-minted headless draft boundary: unresolved target refuses before any write, with no draft, dispatch audit, or open fence.

## 8. Record the admission-source boundary

- [ ] Parent landing ceremony records D-153 in the decision-log index, full decision section, and Change Log: the delivered callers are per-instance approval and the headless approved lane; scope-matched standing-rule admission remains #128; generic shell dispatch fails closed.
- [x] Keep the proposal and tests honest about the absence of a live standing-rule executor caller.

## 9. Verification and landing (authority-sensitive: effect execution and standing-rule budget)

- [x] Run the focused executor, dispatch-boundary, outcome, and readiness tests, including mutation checks for the old stub path and headless bypass.
- [x] Run the repository gate and strict OpenSpec validation; fix any newly exposed false-success tests by asserting fail-closed behavior rather than widening the seven-id allowlist.
- [ ] Land the change, archive it with the resolved OpenSpec binary, validate all archived specs, and run the post-landing gate on `main`.
