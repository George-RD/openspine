# gate-action-api Specification Delta

## ADDED Requirements

### Requirement: Scope-matched admission MUST supply only a kernel-resolved request to the executor

Scope-matched standing-rule admission MUST reach an effect executor only through a kernel-resolved, digest-bound request. It MUST supply the payload reference, target reference, and target digest from the context the kernel itself resolved and the owner reviewed — never from shell-supplied request data, and never reconstructed from an opaque dispatch payload. A shell MUST NOT be able to select or widen any value the executor re-derives against. The executor's re-derivation of payload and target MUST remain load-bearing and MUST run against those kernel-owned values exactly as it does for the per-instance approval and headless approved sources. A resolved context that cannot be constructed MUST fail closed before the executor is reached.

#### Scenario: Scope-matched admission hands over kernel-resolved values

Given a resolved context whose reviewed scope matched exactly one active standing rule
When scope-matched admission dispatches the effect
Then the executor MUST receive the payload ref, target ref, and target digest from the kernel-resolved context
And it MUST re-derive both digests against those values before any provider write.

#### Scenario: Shell cannot supply an executor-bound value

Given a shell-originated request for a scope-matched action
When the admission path assembles the executor request
Then no payload ref, target ref, or target digest may originate in shell-supplied fields
And a shell attempting to supply one MUST NOT widen the reviewed scope.

#### Scenario: Unconstructible context never reaches the executor

Given a resolved context that fails construction because a descriptor, connector, account, required dimension, or bound counterparty is missing
When scope-matched admission is attempted
Then admission MUST fail closed before the executor is invoked
And no provider write MUST be attempted.

## MODIFIED Requirements

### Requirement: Unspecified actions MUST be denied

Actions absent from allowed and approval-required actions MUST be denied. After a gate decision admits an action to dispatch, a catalogued action with no registered handler or executor MUST also fail closed unless the action is explicitly declared as a catalogued non-effect READ stub. Such a miss MUST return the typed `DispatchError::NoExecutor(ActionId)` and MUST NOT report a successful stub. Unknown action ids are never eligible for the non-effect stub path.

Adding scope-matched admission MUST NOT weaken any of these boundaries. `DispatchError::NoExecutor` MUST remain rendered as the same opaque `500 {"error": "internal_error"}` used for every other dispatch failure, so a semi-trusted shell still cannot enumerate which catalogued actions are unwired. The non-effect stub allowlist MUST remain closed at its declared catalogued READ ids, and MUST NOT gain an entry merely because an action became scope-matchable.

#### Scenario: Agent requests unknown action

Given an agent requests `network.raw_egress`
And the task grant does not allow it
When gate() evaluates the request
Then gate() MUST deny the request.

#### Scenario: Catalogued effectful action has no executor

Given the task grant admits the catalogued action `email.send`
And dispatch finds no registered handler or executor for that action
When the kernel dispatch path handles the miss
Then dispatch MUST return `DispatchError::NoExecutor("email.send")`
And it MUST NOT return a successful stub value.

Test: `unregistered_effect_actions_fail_closed_without_stub`

#### Scenario: Declared non-effect read retains the stub boundary

Given the task grant admits the catalogued READ action `memory.read:owner_preferences_limited`
And dispatch finds no registered handler or executor for that action
When the kernel dispatch path handles the miss
Then dispatch MAY return the existing stub shape
And the response MUST identify that no Step 4 kernel-side implementation exists.

Test: `unregistered_known_action_returns_stub_shape`

#### Scenario: Unknown ids are not stub-eligible

Given an unknown action id is absent from the catalog's non-effect stub set
When dispatch finds no registered handler or executor
Then dispatch MUST return `DispatchError::NoExecutor` rather than a stub.

Test: `unknown_action_is_not_stub_eligible`

#### Scenario: Registered effectful paths cannot return success stubs

Given `workflow.invoke:approved` and `setup.workflow.start` are catalogued effectful actions that previously had placeholder registered handlers
When their dispatch paths are exercised after those placeholder handlers are deleted
Then no registered handler MAY return a successful `{"stub": true}` body for either action
And both actions MUST fail closed without a stub
And the kernel MUST render the failure as the same opaque `500 {"error": "internal_error"}` it uses for every other dispatch failure, so a client cannot enumerate which catalogued actions are unwired
And the `/setup` shell path MUST surface that failure rather than diagnosing it — inventing a "not implemented" owner reply from an opaque 500 would mask a genuine connector or resource outage.

Test: `unregistered_effect_actions_fail_closed_without_stub`, `setup_fail_closed_dispatch_surfaces_an_error_and_claims_nothing`

#### Scenario: Scope-matched admission does not widen the dispatch boundary

Given an action that is scope-matchable by an active standing rule but has no registered executor
When scope-matched admission dispatches it
Then dispatch MUST return `DispatchError::NoExecutor`
And the kernel MUST render it as the same opaque `500 {"error": "internal_error"}`
And the action MUST NOT become eligible for the non-effect stub path.
