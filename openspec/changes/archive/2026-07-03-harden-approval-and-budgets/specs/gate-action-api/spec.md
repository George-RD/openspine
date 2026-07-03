# Spec: Gate action API

## ADDED Requirements

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
