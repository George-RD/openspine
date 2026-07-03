# Spec: Gate action API

## Purpose

Define the single mediation point every effectful action must pass through: a typed `gate()` boundary that evaluates an action request against a task grant and returns allow, deny, or approval-required — never a silent side effect.

## Requirements

### Requirement: Every effectful action MUST pass through gate()

OpenSpine MUST mediate every effectful action through gate().

#### Scenario: Agent requests external read

Given an agent requests an external read
When the action is submitted
Then the request MUST pass through gate()
And it MUST be evaluated against the active task grant.

### Requirement: Gate decisions MUST use task grant precedence

gate() MUST apply precedence in this order: explicit deny, approval-required, allow, unspecified deny.

#### Scenario: Action appears in allowed and denied lists

Given an action appears in both allowed and denied actions
When gate() evaluates the request
Then the decision MUST be deny.

#### Scenario: Action appears in allowed and approval-required lists

Given an action appears in both allowed and approval-required actions
When gate() evaluates the request
Then the decision MUST be approval-required.

### Requirement: Unspecified actions MUST be denied

Actions absent from allowed and approval-required actions MUST be denied.

#### Scenario: Agent requests unknown action

Given an agent requests `network.raw_egress`
And the task grant does not allow it
When gate() evaluates the request
Then gate() MUST deny the request.

### Requirement: Gate decisions MUST be auditable

Every gate decision MUST emit or return audit metadata sufficient to record the action, decision, reason, task grant, and target refs.

#### Scenario: Gate denies email send

Given an agent requests `email.send`
When gate() denies the request
Then an audit event MUST record the denial reason
And private payloads MUST be referenced by protected refs rather than plaintext.

### Requirement: Approval-required decisions MUST not execute immediately

If gate() returns approval-required, the action MUST NOT execute until approval is recorded and validated.

#### Scenario: Draft creation requires approval

Given `email.create_draft` is approval-required
When an agent requests draft creation
Then gate() MUST return approval-required
And the connector action MUST NOT execute immediately.

### Requirement: Grant limits MUST be enforced at runtime

`GrantLimits.max_model_calls` and `GrantLimits.max_artifacts` MUST be
enforced, not merely composed and advertised. Enforcement lives in
kernel dispatch — the same placement as selection-token single-use
consumption — not inside the pure `gate()` function.

#### Scenario: Model call beyond the budget

Given a grant with `max_model_calls: N`
And the grant has already made `N` `model.generate` calls
When the shell submits an `(N+1)`th `model.generate` request
Then the kernel MUST deny the request with `DenialReason::LimitExceeded`
And MUST NOT call the model provider.

#### Scenario: Shell-initiated artifact creation beyond the budget

Given a grant with `max_artifacts: N`
And the grant has already created `N` shell-initiated artifact blobs
When the shell triggers an `(N+1)`th shell-initiated artifact put
Then the kernel MUST deny the request with `DenialReason::LimitExceeded`.

### Requirement: Kernel-originated owner notifications are a trusted, audited path

Every kernel-originated owner notification MUST be audited as
`owner.notified`, even though it is not gate-mediated. The kernel MAY
send pipeline-failure or status notices to the grant-bound owner chat
without going through `gate()`, but every agent- or shell-originated
effect MUST remain gate-mediated; this carve-out applies only to
kernel-authored courtesy text, never to agent- or shell-supplied content.

#### Scenario: Kernel sends a courtesy notice

Given a pipeline step fails in a way the owner should know about
When the kernel calls `notify_owner_best_effort`
Then the send MUST NOT be blocked on a `gate()` decision
And an `owner.notified` audit row MUST be appended.
