# Spec: Core runtime schemas

## Purpose

Define explicit, versioned schemas for every core OpenSpine runtime object — event envelope, identity, route, task grant, action request, gate decision, approval, artifact, audit — before runtime implementation relies on them, so identity, routing, and authority stay structurally separated from the start.

## Requirements

### Requirement: OpenSpine core runtime objects MUST have explicit schemas

OpenSpine core runtime objects MUST have explicit schemas before runtime implementation relies on them.

Core runtime objects MUST include event envelope, identity resolution, route artifact, agent manifest, workflow manifest, capability pack, authority composition input/output, task grant, action request, gate decision, approval record, selection token, model request, audit event, and artifact reference.

#### Scenario: Runtime object is added

Given an implementation introduces a new runtime object
When that object participates in routing, authority, action mediation, model access, memory, connector access, audit, or approval
Then the object MUST have an explicit schema
And the schema MUST be versioned.

### Requirement: Event envelopes MUST include source authenticity fields

Every event envelope MUST include source, connector, account role, event type, received timestamp, verified source status, verification method, replay protection status, actor hints, target refs, data classification, lane, and trust context.

#### Scenario: Telegram owner event is normalized

Given a Telegram owner message is received
When the event envelope is created
Then it MUST include source, connector, event type, verified source, verification method, lane, and trust context.

### Requirement: Identity schemas MUST NOT grant runtime authority

Identity records MUST store entity knowledge only.

Identity records MUST NOT directly attach live capability packs, active routes, live tool access, or task grants.

#### Scenario: Known owner identity exists

Given an identity record represents the owner
When a Telegram message is received
Then the identity record MAY contribute relationship and confidence information
But it MUST NOT grant authority by itself.

### Requirement: Route schemas MUST be declarative artifacts

Routes MUST be represented as declarative, versioned artifacts.

Routes MUST map event/context conditions to candidate agent, workflow, and capability pack references.

Routes MUST NOT directly grant final runtime authority.

#### Scenario: Owner Telegram route exists

Given a route matches `telegram.owner.message`
When route resolution succeeds
Then the route MAY select `main_assistant_agent`, `owner_control_conversation`, and `owner_control_basic_pack`
But final authority MUST still be materialized through a task grant.

### Requirement: Route resolution schemas MUST represent ambiguity

Route resolution MUST represent success, denial, and ambiguity.

Ambiguous route matches MUST fall back to low-authority triage or review.

#### Scenario: Two routes match without deterministic winner

Given two active routes match an event
And no deterministic priority or specificity rule selects a winner
When route resolution runs
Then the result MUST be ambiguous
And it MUST NOT grant widened authority.

### Requirement: Task grants MUST be explicit live authority objects

Task grants MUST be short-lived, purpose-bound, route-bound, agent-bound, workflow-bound, and target-bound where applicable.

Running agents and workflows MUST receive a task grant rather than broad permissions.

#### Scenario: Email reply drafter starts

Given a selected-thread email drafting workflow starts
When the workflow is invoked
Then the email reply drafter MUST receive a task grant
And the task grant MUST include allowed, denied, and approval-required actions.

### Requirement: Action requests and gate decisions MUST be typed

Every effectful action MUST be represented as a typed action request.

Every gate result MUST be represented as a typed gate decision.

#### Scenario: Agent requests email thread read

Given an agent requests to read an email thread
When the request reaches gate()
Then gate() MUST evaluate a typed action request against the task grant
And return an allow, deny, or approval-required decision.

### Requirement: Approval records MUST bind reviewed payloads and targets

Approval records MUST bind the exact reviewed payload digest and target digest.

Any mutation to body, recipient, target, thread, or payload MUST invalidate approval.

#### Scenario: Draft body changes after approval

Given the user approved a draft body
When the body changes before execution
Then the prior approval MUST NOT authorize the changed payload.

### Requirement: Audit schemas MUST reference private payloads by encrypted or hash refs

Audit events MUST store metadata directly.

Private payloads MUST be stored as encrypted artifact refs, hash refs, or equivalent protected references rather than raw plaintext audit text.

#### Scenario: Model request includes private email content

Given a model request includes private email context
When audit is written
Then raw private content MUST NOT be written directly into the audit event.
And the audit event MUST reference protected artifact refs and hashes.
