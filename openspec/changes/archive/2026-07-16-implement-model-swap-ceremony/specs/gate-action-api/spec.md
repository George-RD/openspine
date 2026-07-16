# Spec: Gate action API

## MODIFIED Requirements

### Requirement: ActionCatalog MUST enumerate every trusted-path carve-out around gate()

Every effectful path that reaches around `gate()` — whether `gated-shell`, `post-gate-approved-effect`, `kernel-origin-gated`, or `internal-maintenance-non-effect` — MUST be enumerated as data in the `ActionCatalog` as a classified entry, and each enumerated entry MUST have a dedicated characterization test asserting its gate-decision and audit-event behavior (D-055.1).

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
