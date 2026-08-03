# Spec: Core runtime schemas

## MODIFIED Requirements

### Requirement: OpenSpine core runtime objects MUST have explicit schemas

OpenSpine core runtime objects MUST have explicit schemas before runtime implementation relies on them.

Core runtime objects MUST include event envelope, identity resolution, route artifact, agent manifest, workflow manifest, capability pack, authority composition input/output, task grant, action request, gate decision, approval record, selection token, model request, audit event, artifact reference, principal, action descriptor, action implementation descriptor, resolved action context, reviewed action scope, delegation evidence, owner review request, and responsibility manifest.

#### Scenario: Runtime object is added

Given an implementation introduces a new runtime object
When that object participates in routing, authority, action mediation, model access, memory, connector access, audit, approval, or reusable-delegation review
Then the object MUST have an explicit schema
And the schema MUST be versioned.
