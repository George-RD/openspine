# Spec: Authority composition

## ADDED Requirements

### Requirement: Authority composition MUST be deny-by-default

Authority composition MUST start from deny-by-default.

Actions MUST NOT be allowed unless they are permitted by all required authority sources.

#### Scenario: No candidate allow exists

Given an event resolves to a route
And no applicable authority source allows `email.read_inbox`
When authority composition runs
Then `email.read_inbox` MUST NOT be included in allowed actions.

### Requirement: Task grant MUST be the final authority object

The authority composer MUST materialize final runtime authority as a task grant.

Agents and workflows MUST NOT receive raw route, capability pack, policy, or identity objects as live authority.

#### Scenario: Owner control route resolves

Given `telegram.owner.message` resolves to the owner-control route
When authority composition succeeds
Then the output MUST include a task grant
And the running agent MUST be constrained by that task grant.

### Requirement: Explicit deny MUST override allow

If an action is both allowed by one source and denied by another, the action MUST be denied.

#### Scenario: Capability pack allows but global policy denies

Given a capability pack allows `email.send`
And global policy denies `email.send`
When authority composition runs
Then `email.send` MUST be denied.

### Requirement: Approval-required MUST override plain allow

If an action is allowed by one source and approval-required by another source, the action MUST require approval.

#### Scenario: Workflow allows draft creation but policy requires approval

Given a workflow allows `email.create_draft`
And policy marks `email.create_draft` approval-required
When authority composition runs
Then `email.create_draft` MUST appear in approval-required actions
And MUST NOT execute without approval.

### Requirement: Identity MUST NOT grant authority by itself

Identity resolution output MUST NOT grant live task authority by itself.

#### Scenario: Owner identity matches but source is unverified

Given an event has an actor hint matching the owner
And the event source is not verified
When authority composition runs
Then owner authority MUST NOT be granted.

### Requirement: Connector and account role MUST NOT grant authority by themselves

Connector authentication and account role MUST contribute constraints and risk posture but MUST NOT grant authority by themselves.

#### Scenario: Gmail connector is authenticated

Given the Gmail connector is authenticated
And no selected-thread token exists
When authority composition runs
Then selected-thread read authority MUST NOT be granted.

### Requirement: Main assistant grant MUST NOT inherit specialist workflow authority

The main assistant MUST NOT automatically inherit permissions from workflows it can invoke.

#### Scenario: Main assistant invokes email drafting workflow

Given `main_assistant_agent` can invoke approved workflows
When it invokes selected-thread email drafting
Then the main assistant task grant MUST NOT directly include selected email read authority
And the specialist workflow MUST receive its own separate task grant.

### Requirement: Authority widening MUST require explicit approval

Changes that widen authority MUST require explicit human approval before activation.

#### Scenario: Proposal adds inbox-wide read

Given a proposed change adds inbox-wide read authority
When authority composition evaluates the current task
Then the proposed authority MUST NOT be active
And activation MUST require explicit human approval.
