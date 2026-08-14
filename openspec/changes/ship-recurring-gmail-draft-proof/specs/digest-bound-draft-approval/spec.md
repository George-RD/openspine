## MODIFIED Requirements

### Requirement: The kernel MUST re-derive digests from artifact-store bytes at approval-effect time

Shell-facing request DTOs MUST structurally be unable to carry digest fields; the shell sends intents and the kernel computes outcomes. For draft approval, the kernel MUST load the content-addressed payload bytes and re-derive the payload and target semantics at effect time before allowing the effect, matching the existing D-055.4 path. For plan approval, the kernel MUST load the content-addressed plan bytes and re-derive `Plan::digest()` before persisting `ApprovalRecord`, compare it with the request-bound payload digest, and re-derive it again at resolution before recording the plan resolution. The plan target digest is kernel-authored at proposal time and remains integrity-protected by the persisted `ActionRequest`; this requirement does not claim a separate re-derivation of the plan target digest.

The shared kernel-owned Gmail draft executor MUST preserve every draft re-derivation and the pending-write fence, and MUST return a truthful typed outcome: `RefusedPreEffect` when a known refusal occurs before a provider write, `DeliveryUnknown` when the write outcome cannot be determined, `FailedAfterAttempt` when a write was attempted and failed without an unknown-delivery classification, and `Executed` only after the provider confirms creation and the existing post-write evidence completes. The delivered approved-admission sources MUST call this same executor; callers MUST NOT collapse an uncertain or refused outcome into a dispatched-success audit.

Scope-matched standing-rule admission becomes a third caller of that same executor. It MUST supply the payload ref, target ref, and target digest from the kernel-resolved reviewed context rather than from any shell-supplied value, and the executor MUST remain unchanged: every re-derivation, the pending-write fence, and the ordering that takes the connector write permit BEFORE recording the fence all stay as they are. Unlike the two per-instance approval callers, scope-matched admission holds a reserved standing-rule budget across the effect, so it MUST additionally map the returned outcome onto that reservation: `Executed` finalizes it, `DeliveryUnknown` retains it with the reconciliation fence left open, and `RefusedPreEffect` or `FailedAfterAttempt` cancels it without consuming budget.

Retry fencing for kernel-resolved Gmail draft requests is specified by the `standing-rules` requirement "Pending Gmail writes MUST fence retries before budget reservation"; this capability adds no second predicate.

#### Scenario: Stored plan bytes mutate after question presentation

Given a pending plan question whose request carries the original plan digest
When approval-effect handling loads stored bytes that deserialize to a different plan
Then it MUST refuse before persisting an approval or resolving the plan.

#### Scenario: Exact stored plan bytes are approved

Given a pending plan question and content-addressed bytes whose re-derived digest matches the bound digest
When the owner taps the plan approval callback
Then the kernel MUST persist the digest-bound approval and re-run `gate()` before recording plan resolution.

#### Scenario: Shell cannot supply a plan digest

Given a shell-facing plan proposal payload
When its fields are inspected
Then it MUST NOT be able to select the approval digest independently of the canonical stored plan bytes.

#### Scenario: Payload bytes changed after approval

Given an approved `email.create_draft` request
When the kernel re-derives the payload digest from the artifact store at effect time
And the re-derived digest differs from the approved payload digest
Then the kernel MUST deny draft creation
And the shared executor MUST return `EffectOutcome::RefusedPreEffect` without attempting a provider write.

Test: `payload_mutated_since_approval_is_denied_and_creates_no_draft`

#### Scenario: Target digest re-derivation mismatch denies

Given an approved `email.create_draft` request
When the kernel re-derives the target digest (`approval.rs:290`) and it differs from the approved target digest
Then the kernel MUST deny draft creation
And the shared executor MUST return `EffectOutcome::RefusedPreEffect` without attempting a provider write.

Test: `target_mutated_since_approval_is_refused_without_a_draft`

#### Scenario: Shell cannot supply a digest

Given a shell-facing request DTO
When its fields are inspected
Then it MUST NOT carry a payload or target digest field
And the kernel MUST compute every digest from store bytes.

#### Scenario: Delivery is unknown after a provider write attempt

Given an approved `email.create_draft` request whose pending-write fence was inserted
When the Gmail write returns an unknown delivery outcome
Then the shared executor MUST return `EffectOutcome::DeliveryUnknown`
And the pending-draft row MUST remain open for reconciliation
And no `headless.approved_dispatched` audit MUST be emitted for that outcome.

Test: `draft_write_timeout_is_delivery_unknown_and_leaves_pending_row`

#### Scenario: Confirmed creation is executed exactly once in evidence

Given an approved `email.create_draft` request whose payload and target re-derivations match
When the shared executor confirms the Gmail draft write
Then it MUST resolve the pending-draft row
And it MUST emit the existing `draft.created` audit with kernel-derived digests
And it MUST return `EffectOutcome::Executed`.

Test: `successful_draft_write_is_executed_and_resolves_pending_row`

#### Scenario: Post-attempt failure is not reported as success

Given an approved `email.create_draft` request whose provider write was attempted
When post-attempt processing reports a non-unknown failure
Then the shared executor MUST return `EffectOutcome::FailedAfterAttempt`
And it MUST retain the existing failure audit/batch evidence
And it MUST NOT emit a dispatched-success audit.

Test: `definite_write_failure_is_failed_after_attempt_and_resolves_the_fence`

#### Scenario: Approved admission sources converge on one executor

Given an approved digest-bound `email.create_draft` request with reviewed target and payload refs
When per-instance approval and headless approval dispatch the same request
Then both routes MUST invoke the same kernel-owned Gmail executor
And successful routes MUST produce identical `draft.created` action, target refs, and payload refs
And a non-`Executed` headless outcome MUST append zero `headless.approved_dispatched` audits.

Test: `headless_and_non_headless_approval_converge_on_gmail_executor`, `headless_refusal_appends_no_dispatched_audit`

#### Scenario: Webhook-minted headless draft refuses an unresolved target

Given `run_headless_hook` mints an approved digest-bound `email.create_draft` request with `target_ref: None` and the raw webhook body as payload
When the headless approved lane invokes the shared executor
Then target re-derivation MUST return `EffectOutcome::RefusedPreEffect` before any provider write
And no `draft.created` or `headless.approved_dispatched` audit MUST be emitted
And no reconciliation fence MUST remain open.

Test: `webhook_minted_headless_draft_refuses_before_any_write`

#### Scenario: Write admission precedes pending-write fence recording

Given the Gmail write's admission permit is rejected before any write future is polled
When the shared executor attempts the draft write
Then it MUST take the write permit before recording the pending-write fence
And it MUST return `EffectOutcome::RefusedPreEffect`
And no provider write MUST be attempted and no pending-write fence row MUST exist
And the read-only recipient re-derivation MAY already have read the Gmail thread — a read is not an effect, so "nothing was sent" is the claim, not "nothing was called".

Test: `rate_limited_write_admission_is_refused_without_a_fence_row`, `unavailable_gmail_connector_refuses_before_any_fence_row`

#### Scenario: Scope-matched admission is the third caller of the same executor

Given a resolved context whose reviewed scope matched exactly one active standing rule for `email.create_draft`
When scope-matched admission dispatches the effect
Then it MUST invoke the same kernel-owned Gmail executor the two approval sources use
And it MUST supply the payload ref, target ref, and target digest from the kernel-resolved context
And the executor MUST perform the same payload and target re-derivations and take its write permit before recording the pending-write fence.

#### Scenario: Executed finalizes the reserved standing-rule budget

Given scope-matched admission that reserved quota and rate before dispatch
When the executor returns `EffectOutcome::Executed`
Then the reservation MUST be finalized as committed usage
And the existing `draft.created` evidence MUST be emitted exactly once.

#### Scenario: Delivery-unknown retains the reserved budget and the fence

Given scope-matched admission that reserved quota and rate before dispatch
When the executor returns `EffectOutcome::DeliveryUnknown`
Then the reservation MUST be retained rather than cancelled or finalized
And the pending-draft row MUST remain open for reconciliation
And no dispatched-success audit MUST be emitted.

#### Scenario: Refusal and post-attempt failure release the reserved budget

Given scope-matched admission that reserved quota and rate before dispatch
When the executor returns `EffectOutcome::RefusedPreEffect` or `EffectOutcome::FailedAfterAttempt`
Then the reservation MUST be cancelled
And no quota or rate budget MUST be consumed.

#### Scenario: Delivery is unknown and retry is fenced

Given an approved `email.create_draft` write has a pending row because delivery is unknown
And the same kernel-resolved request is retried
When the retry reaches the action API
Then it MUST be blocked before provider execution and scoped budget reservation
And the pending row MUST remain open for reconciliation
And owner review MUST remain the next authority step.

#### Scenario: Exact request after explicit resolution may proceed

Given the pending row has been explicitly resolved after reconciliation
And the new request's kernel-derived payload and target digests match
When the retry reaches the action API
Then it MAY proceed through the ordinary gate and shared executor.
