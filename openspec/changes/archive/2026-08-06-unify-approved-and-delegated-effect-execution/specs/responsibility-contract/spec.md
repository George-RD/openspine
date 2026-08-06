# responsibility-contract Specification

## MODIFIED Requirements

### Requirement: Reusable delegation MUST validate independent action and implementation declarations

The kernel MUST require a complete catalog-owned action descriptor and a complete concrete implementation descriptor before a reusable-delegation proposal reaches owner review. The implementation MUST identify a resolver and executor with explicit versions. Executor readiness MUST additionally require that the descriptor's `executor_id` is registered in the kernel-owned effect-executor registry for the action. A descriptor alone MUST NOT prove that the action is runnable. Missing or mismatched action, resolver, implementation, or executor declarations MUST fail closed.

#### Scenario: Semantic descriptor exists but executor does not

- **WHEN** `email.create_draft` has reviewed semantics but no reusable implementation descriptor
- **THEN** delegation readiness MUST return a typed missing-implementation error
- **AND** no owner proposal may claim that the reusable effect path is ready
- **AND** the typed error MUST be `MissingImplementationDescriptor`

#### Scenario: Descriptor exists but executor is not registered

- **WHEN** `email.create_draft` has reviewed semantics and a complete implementation descriptor but its declared `gmail.create_draft` executor is not registered
- **THEN** execution readiness MUST return false
- **AND** no owner proposal may claim that the reusable effect path is ready
- **AND** dispatch MUST NOT return a successful stub.

Test: `is_execution_backed_requires_descriptor_and_registered_executor`

#### Scenario: Descriptor and registered executor establish readiness

- **WHEN** `email.create_draft` has its action-keyed D-146 descriptor and the declared `gmail.create_draft` executor is registered
- **THEN** execution readiness MUST return true
- **AND** the readiness result MUST identify the descriptor-plus-registry conjunction rather than a separate effect-class enum.
