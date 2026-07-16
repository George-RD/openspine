# Spec: Core runtime schemas

## MODIFIED Requirements

### Requirement: OpenSpine core runtime objects MUST have explicit schemas

OpenSpine core runtime objects MUST have explicit schemas before runtime implementation relies on them.

Core runtime objects MUST include event envelope, identity resolution, route artifact, agent manifest, workflow manifest, capability pack, authority composition input/output, task grant, action request, gate decision, approval record, selection token, model request, audit event, artifact reference, and principal.

#### Scenario: Runtime object is added

Given an implementation introduces a new runtime object
When that object participates in routing, authority, action mediation, model access, memory, connector access, audit, or approval
Then the object MUST have an explicit schema
And the schema MUST be versioned.

### Requirement: Identity schemas MUST NOT grant runtime authority

Identity and Principal records MUST store entity knowledge only.

Identity and Principal records MUST NOT directly attach live capability packs, active routes, live tool access, or task grants.

Identity resolution MUST return an optional principal_id that is Some only for the owner in v1.

#### Scenario: Known owner identity exists

Given an identity record represents the owner
When a Telegram message is received
Then the identity record MAY contribute relationship and confidence information
But it MUST NOT grant authority by itself.
