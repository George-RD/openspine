# Design: Workflow state machines

## Manifest shape

`WorkflowManifest` retains the legacy `steps: Vec<String>` field and adds optional `initial_state`, `states`, and `transitions`. A state contains typed deterministic/agentic steps, an optional escalation point, and approval semantics. Approval-required states declare the exact `approval_action`. Each typed step declares `ReasoningTier` (`low`, `standard`, or `high`). Legacy YAML and Rust fixtures deserialize with empty state-machine fields and use the standard tier fallback.

`WorkflowManifest::validate` rejects empty or duplicate state/step ids, missing initial state when any declarative machine fields are present, missing initial-state references, transitions to undeclared states, and approval-required states without an action. It also rejects approval-required → approval-required edges because the current one-request transition API cannot bind two distinct approval gates. `to_mermaid` uses an injective, alphabetic-prefixed encoding for exact directed transitions without introducing authority semantics.

## Runtime

`WorkflowStateMachine` validates and rehydrates a manifest through `WorkflowCtx::new_with_definition`. It binds the complete serialized manifest digest at run start and verifies it on resume, then reconstructs the latest completed transition target and explicitly advances the replay cursor for its own wrapper; ordinary `WorkflowCtx` callers retain cursor-zero replay behavior.

Each transition writes exactly one advancing durable step. Entering an approval-required state validates and persists the request id, action, payload digest, and target digest in one `workflow.entry_binding` step. Leaving an approval-required state loads the immutable `ActionRequest` and matching `ApprovalRecord` from `Store`; the request action must equal the state declaration, both payload and target digests must be present and match D-011, and the approval must be approved and unexpired. Authorization happens before a new durable step is appended. The approval-gated departure is one non-generic `workflow.approval` adapter whose non-secret `TransitionOutcome` is replayable and whose input binds the exact edge and request id. Non-approval transitions use a typed `workflow.transition` step.

## Gateway routing

`GatewayTierMap` stores only explicit tier-to-provider overrides. Resolution takes the current active provider id at call time as fallback, so a later approved model swap cannot leave standard routing pinned to a stale client. The gateway endpoint uses the standard tier by default; workflow callers resolve each declared step tier through the same map. No synthetic provider JSON fields are emitted. Production workflow driving and threading declared tiers through worker execution are intentionally deferred to the `worker-runtime` / `seed-workflows` changes; this change's done-when is the substrate enforcement and routing tests.

## Security and replay

The workflow ledger stores only state ids, digests, ids, and closed transition outcomes. Approval authorization is Store-backed and digest-bound; denied authorization performs no workflow append. A crash between an approval entry/departure intent and completion leaves the typed pending step for deterministic resumption, while a completed transition rehydrates both state and cursor before the next transition. Malformed completed reserved outcomes fail closed rather than being skipped.
