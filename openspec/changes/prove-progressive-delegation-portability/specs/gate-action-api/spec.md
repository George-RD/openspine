## MODIFIED Requirements

### Requirement: Scope-matched admission MUST supply only a kernel-resolved request to the executor


Scope-matched standing-rule admission MUST reach an effect executor only through a kernel-resolved, digest-bound request. It MUST supply the payload reference, target reference, and target digest from the context the kernel itself resolved and the owner reviewed — never from shell-supplied request data, and never reconstructed from an opaque dispatch payload. A shell MUST NOT be able to select or widen any value the executor re-derives against. The executor's re-derivation of payload and target MUST remain load-bearing and MUST run against those kernel-owned values exactly as it does for the per-instance approval and headless approved sources. A resolved context that cannot be constructed MUST fail closed before the executor is reached.

This holds per shape rather than per action id. A second shape's executor MUST be reached only through its own catalogued `implementation_id`/`executor_id`, and every payload reference, target reference, target digest, and visibility value it receives MUST come from that shape's kernel-side resolver rather than from any shell-supplied field. An unregistered executor for a declared shape MUST remain a typed `NoExecutor` that cancels the reservation instead of reporting a successful stub, and one shape's executor MUST NOT be reachable with another shape's resolved context.

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

#### Scenario: A shape's executor rejects another shape's resolved context

- **WHEN** a resolved context for one communication shape is directed at another shape's registered executor
- **THEN** dispatch MUST fail closed
- **AND** no effect may be attempted

#### Scenario: A declared shape with no registered executor cancels rather than stubs

- **WHEN** a scope-matched second-shape admission resolves an `executor_id` that is not registered
- **THEN** dispatch MUST return the typed `NoExecutor` outcome
- **AND** the reservation MUST be cancelled and the full reviewed budget restored

