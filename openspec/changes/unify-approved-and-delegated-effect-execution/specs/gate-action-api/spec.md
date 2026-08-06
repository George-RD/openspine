# Spec: Gate action API

## MODIFIED Requirements

### Requirement: Unspecified actions MUST be denied

Actions absent from allowed and approval-required actions MUST be denied. After a gate decision admits an action to dispatch, a catalogued action with no registered handler or executor MUST also fail closed unless the action is explicitly declared as a catalogued non-effect READ stub. Such a miss MUST return the typed `DispatchError::NoExecutor(ActionId)` and MUST NOT report a successful stub. Unknown action ids are never eligible for the non-effect stub path.

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

### Requirement: ActionCatalog MUST enumerate every trusted-path carve-out around gate()

Every effectful path that reaches around `gate()` — whether `gated-shell`,
`post-gate-approved-effect`, `kernel-origin-gated`, or `internal-maintenance-non-effect` —
MUST be enumerated as data in the `ActionCatalog` as a classified entry, and each enumerated entry MUST have a dedicated characterization test asserting its gate-decision and audit-event behavior (D-055.1).

The complete classified inventory is:

| # | Effect path | Classification |
|---|-------------|----------------|
| 1 | `notify_owner_best_effort` | `kernel-origin-gated` |
| 2 | `notify_owner_required` | `kernel-origin-gated` |
| 3 | `create_approved_draft` | `post-gate-approved-effect` |
| 4 | `activate_approved_artifact` | `post-gate-approved-effect` |
| 5 | `dispatch_read_selected_thread` | `gated-shell` |
| 6 | `dispatch_lyra_preview/propose_draft_creation` | `gated-shell` |
| 7 | `dispatch_artifact_propose` | `gated-shell` |
| 8 | `run_model_swap_golden_set` | `gated-shell` |
| 9 | `apply_model_swap_activation` | `post-gate-approved-effect` |
| 10 | `dispatch_plan_preview` | `gated-shell` |
| 11 | `resolve_approved_plan` | `post-gate-approved-effect` |
| 12 | `sweep_expired_grants` | `internal-maintenance-non-effect` |
| 13 | `answer_callback_query` | `internal-maintenance-non-effect` |

The catalog MUST also expose the execution boundary for actions that can reach generic dispatch. A catalogued action with a missing handler or executor MUST fail closed with `DispatchError::NoExecutor(ActionId)` unless it is explicitly declared in the catalog's non-effect stub set. That set MUST contain only catalogued READ actions with no kernel-side implementation and no dedicated production route; every write, mutation, external effect, and unknown id MUST remain outside it.

#### Scenario: The carve-out set is finite and enumerated

Given the `ActionCatalog`
When the trusted-path carve-outs are enumerated
Then exactly the thirteen classified entries above MUST exist
And no effectful path outside the catalog MAY reach a side effect.

#### Scenario: Each enumerated entry has a dedicated test

Given the thirteen enumerated effect paths
When the kernel test suite is inspected
Then each entry MUST have at least one dedicated characterization test asserting its gate decision and corresponding audit event (including `action.gated` for gate-mediated paths and the applicable effect audit for post-gate paths).

#### Scenario: Model golden-set execution is classified

Given a model-swap proposal requests golden-set execution
When the request is submitted
Then `run_model_swap_golden_set` MUST be catalogued as `gated-shell`
And a characterization test MUST assert the gate decision and `action.gated` audit before the provider is called.

#### Scenario: Model activation is classified

Given an approved model-swap proposal is activated
When the post-approval effect runs
Then `apply_model_swap_activation` MUST be catalogued as `post-gate-approved-effect`
And a characterization test MUST assert the approval gate decision and activation audit.

#### Scenario: An unregistered effectful action fails closed

Given a catalogued action has no registered handler or executor
And it is not one of the catalogued non-effect READ ids
When generic dispatch reaches the registry miss
Then the kernel MUST return `DispatchError::NoExecutor(ActionId)`
And MUST NOT report a successful stub.

Test: `unregistered_effect_actions_fail_closed_without_stub`

#### Scenario: Only declared non-effect reads may use the stub

Given a catalogued READ action is in the non-effect stub set
When generic dispatch reaches a registry miss
Then the kernel MAY return the existing stub shape
And a catalogued write or mutation MUST return `DispatchError::NoExecutor` instead.

Test: `unregistered_known_action_returns_stub_shape`, `unregistered_effect_actions_fail_closed_without_stub`
