# Spec: Gate action API

## ADDED Requirements

### Requirement: ActionCatalog MUST enumerate every trusted-path carve-out around gate()

Every effectful path that reaches around `gate()` — whether `gated-shell`,
`post-gate-approved-effect`, `kernel-origin-gated`, or `internal-maintenance-non-effect` —
MUST be enumerated as data in the `ActionCatalog` as a classified entry, and each
enumerated entry MUST have a dedicated characterization test asserting its
gate-decision and audit-event behavior. (Settles design decision D-055.1.)

The eight enumerated effect paths and their classifications:

| # | Effect path | File:line | Classification |
|---|-------------|-----------|----------------|
| 1 | `notify_owner_best_effort` | `crates/openspine-kernel/src/pipeline/mod.rs:147-157` | `kernel-origin-gated` |
| 2 | `create_approved_draft` | `crates/openspine-kernel/src/pipeline/approval.rs:206-359` | `post-gate-approved-effect` |
| 3 | `activate_approved_artifact` | `crates/openspine-kernel/src/pipeline/approval.rs:367-476` | `post-gate-approved-effect` |
| 4 | `dispatch_read_selected_thread` | `crates/openspine-kernel/src/api/actions.rs:367-441` | `gated-shell` (token-validated in `gate()`) |
| 5 | `dispatch_lyra_preview` / `propose_draft_creation` | `crates/openspine-kernel/src/api/actions.rs:225-358` | `gated-shell` |
| 6 | `dispatch_artifact_propose` | `crates/openspine-kernel/src/api/artifact_propose.rs:37-184` | `gated-shell` |
| 7 | `sweep_expired_grants` | `crates/openspine-kernel/src/store/budget_support.rs:94-106` | `internal-maintenance-non-effect` |
| 8 | `answer_callback_query` | `crates/openspine-kernel/src/pipeline/approval.rs:46`; `pipeline/mod.rs:238` | `internal-maintenance-non-effect` |

No effect path MAY bypass `gate()` without an enumerated, classified, and tested entry.

#### Scenario: The carve-out set is finite and enumerated

Given the `ActionCatalog`
When the trusted-path carve-outs are enumerated
Then exactly the classified entries above MUST exist
And no effectful path outside the catalog MAY reach a side effect.

#### Scenario: Each enumerated entry has a dedicated test

Given the eight enumerated effect paths
When the kernel test suite is inspected
Then each entry MUST have at least one characterization test asserting its gate-decision and audit-event behavior.

### Requirement: Kernel-origin actions MUST route through gate() with a KernelOrigin marker

A new `ActionOrigin::{Shell, Kernel}` marker MUST distinguish shell-initiated
intents from kernel-initiated effects. Kernel-originated actions enumerated in the
trusted-origin set (see "ActionCatalog MUST enumerate every trusted-path carve-out
around gate()") MUST be routed through `gate()` with the `Kernel` origin: they are
exempt from approval (auto-allowed) but NEVER from audit — `gate()` MUST emit
`AuditMeta` for every kernel-origin decision. A kernel-origin call for an action
outside the enumerated trusted-origin set MUST be denied. (Settles D-055.2;
generalizes the single `owner.notified` carve-out of D-046.)

#### Scenario: Kernel notify routes through gate but is exempt from approval

Given `notify_owner_best_effort` is invoked by the kernel for a pipeline notice
When `gate()` is called with `ActionOrigin::Kernel` and the `owner.notify` action
Then `gate()` MUST NOT require an approval record
And MUST emit an `owner.notified` audit event via `AuditMeta`.

#### Scenario: Kernel-origin call outside the enumerated set is denied

Given a kernel-initiated request for an action not in the trusted-origin set
When `gate()` is called with `ActionOrigin::Kernel`
Then `gate()` MUST deny the request.

### Requirement: Selection-token validation MUST occur inside gate() for token-requiring actions

For catalog-marked `token_requiring` actions, `gate()` (pure, no I/O) MUST validate
the selection token using `GateContext::find_selection_token`, checking that the
token is bound to the requesting grant, exists, has the expected token type, and is
not expired. The atomic single-use CONSUME of the token remains at dispatch
(after `gate()` returns allow) to preserve `gate()`'s purity — `gate()` never
mutates state. (Settles D-055.3; moves the validation site from
`crates/openspine-kernel/src/api/actions.rs:384-421` into the pure decision.)

#### Scenario: Gate validates a bound, live, unexpired token

Given an action request whose catalog entry requires a selection token
And the token is bound to the requesting grant, exists, has the correct type, and is unexpired
When `gate()` evaluates the request
Then `gate()` MUST allow the request (subject to other grant checks).

#### Scenario: Gate denies a missing, foreign, wrong-type, or expired token

Given an action request whose catalog entry requires a selection token
And the token is missing, bound to a different grant, of the wrong type, or expired
When `gate()` evaluates the request
Then `gate()` MUST deny the request.

#### Scenario: Consumption stays at dispatch

Given `gate()` has allowed a token-requiring request
When the kernel dispatches the effect
Then the atomic single-use token consume MUST occur at dispatch
And `gate()` itself MUST NOT have mutated token state.

## MODIFIED Requirements

### Requirement: Every effectful action MUST pass through gate()

OpenSpine MUST mediate every effectful action through gate(). Every path that
reaches around `gate()` MUST be an enumerated, classified, and tested carve-out in
the `ActionCatalog` (see "ActionCatalog MUST enumerate every trusted-path carve-out
around gate()"); the default rule is that all shell- and agent-initiated effects
pass through `gate()`, and the only non-shell effects are the enumerated
`kernel-origin-gated` entries routed through `gate()` with a `Kernel` origin.

#### Scenario: Agent requests external read

Given an agent requests an external read
When the action is submitted
Then the request MUST pass through gate()
And it MUST be evaluated against the active task grant.

#### Scenario: Every carve-out is mediated or enumerated

Given any effectful path in the kernel
When its mediation is inspected
Then it MUST either pass through `gate()` or be an enumerated carve-out entry that
still routes through `gate()` with a `Kernel` origin.

### Requirement: Gate decisions MUST be auditable

Every gate decision MUST emit or return audit metadata sufficient to record the
action, decision, reason, task grant, and target refs. This holds WITHOUT exception
for kernel-origin actions: a trusted-origin kernel effect routed through `gate()` is
exempt from approval but is NEVER exempt from audit — its `AuditMeta` MUST be
emitted. (Refines the D-046 trusted-path carve-out.)

#### Scenario: Gate denies email send

Given an agent requests `email.send`
When gate() denies the request
Then an audit event MUST record the denial reason
And private payloads MUST be referenced by protected refs rather than plaintext.

#### Scenario: Kernel-origin notify is still audited

Given the kernel routes `notify_owner_best_effort` through `gate()` with `Kernel` origin
When the decision is made
Then an `owner.notified` audit event MUST be emitted even though no approval was required.

### Requirement: Approval-required decisions MUST not execute immediately

If gate() returns approval-required, the action MUST NOT execute until approval is
recorded and validated. Kernel-origin actions enumerated in the trusted-origin set
are exempt from approval (auto-allowed) but are still verified and audited by
`gate()`.

#### Scenario: Draft creation requires approval

Given `email.create_draft` is approval-required
When an agent requests draft creation
Then gate() MUST return approval-required
And the connector action MUST NOT execute immediately.

#### Scenario: Kernel-origin action needs no approval but is audited

Given a kernel-origin action in the trusted-origin set
When `gate()` evaluates it with `ActionOrigin::Kernel`
Then `gate()` MAY auto-allow without an approval record
And MUST still emit `AuditMeta`.

### Requirement: Grant limits MUST be enforced at runtime

`GrantLimits.max_model_calls` and `GrantLimits.max_artifacts` MUST be enforced, not
merely composed and advertised. Enforcement of grant limits remains in kernel
dispatch (per D-046/D-050 atomic-upsert placement). Selection-token *validation*
now lives inside the pure `gate()` function for token-requiring actions (see
"Selection-token validation MUST occur inside gate() for token-requiring actions");
only the atomic single-use token *consumption* remains at dispatch, because
`gate()` MUST stay free of state mutation.

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

The trusted-path carve-out is generalized into an enumerated `KernelOrigin` action
set in the `ActionCatalog` (see "ActionCatalog MUST enumerate every trusted-path
carve-out around gate()"). Every kernel-originated effect in that set MUST be
routed through `gate()` with `ActionOrigin::Kernel`: exempt from approval, never
from audit. The canonical entry is `notify_owner_best_effort`, audited as
`owner.notified`. A kernel-origin call for an action outside the enumerated set
MUST be denied. This generalizes the single `owner.notified` courtesy-notice
carve-out of D-046 into a data-described trusted-origin set.

#### Scenario: Kernel sends a courtesy notice

Given a pipeline step fails in a way the owner should know about
When the kernel calls `notify_owner_best_effort`
Then the send MUST route through `gate()` with `Kernel` origin (no approval required)
And an `owner.notified` audit row MUST be appended.
