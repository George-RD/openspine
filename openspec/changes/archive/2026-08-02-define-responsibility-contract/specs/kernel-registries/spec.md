# kernel-registries Specification

## ADDED Requirements

### Requirement: Reusable-delegation readiness MUST be an explicit catalog axis

The canonical action catalog MUST store protocol-neutral action descriptors independently from concrete action implementation descriptors. The catalog MUST fail closed when an action is unknown, an action descriptor is missing, an implementation descriptor is missing, the declarations disagree, or delegation-policy validation fails.

#### Scenario: Known action lacks reusable implementation

Given `email.create_draft` is a known action with a semantic delegation descriptor
And no shared reusable Gmail resolver/executor has been registered
When delegation readiness is requested
Then the catalog MUST return a typed missing-implementation error
And current per-instance approved draft creation MUST remain unchanged.
