# kernel-registries Specification

## ADDED Requirements

### Requirement: Reusable-delegation readiness MUST be an explicit catalog axis

The canonical action catalog MUST store protocol-neutral action descriptors independently from concrete action implementation descriptors. When reusable-delegation readiness is requested, the catalog MUST fail closed if an action is unknown, an action descriptor is missing, an implementation descriptor is missing, the declarations disagree, or delegation-policy validation fails. This readiness failure MUST NOT change composition or direct-dispatch stub behavior for known-but-unimplemented actions.

#### Scenario: Known action lacks reusable implementation

Given `email.create_draft` is a known action with a semantic delegation descriptor
And no shared reusable Gmail resolver/executor has been registered
When reusable-delegation readiness is requested
Then the catalog MUST return a typed missing-implementation error
And current per-instance approved draft creation MUST remain unchanged.
