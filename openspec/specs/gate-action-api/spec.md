# Spec: Gate action API

## Purpose

Define the single mediation point every effectful action must pass through: a typed `gate()` boundary that evaluates an action request against a task grant and returns allow, deny, or approval-required — never a silent side effect.

## Requirements

### Requirement: Every effectful action MUST pass through gate()

OpenSpine MUST mediate every effectful action through gate().

#### Scenario: Agent requests external read

Given an agent requests an external read
When the action is submitted
Then the request MUST pass through gate()
And it MUST be evaluated against the active task grant.

### Requirement: Gate decisions MUST use task grant precedence

gate() MUST apply precedence in this order: explicit deny, approval-required, allow, unspecified deny.

#### Scenario: Action appears in allowed and denied lists

Given an action appears in both allowed and denied actions
When gate() evaluates the request
Then the decision MUST be deny.

#### Scenario: Action appears in allowed and approval-required lists

Given an action appears in both allowed and approval-required actions
When gate() evaluates the request
Then the decision MUST be approval-required.

### Requirement: Unspecified actions MUST be denied

Actions absent from allowed and approval-required actions MUST be denied.

#### Scenario: Agent requests unknown action

Given an agent requests `network.raw_egress`
And the task grant does not allow it
When gate() evaluates the request
Then gate() MUST deny the request.

### Requirement: Gate decisions MUST be auditable

Every gate decision MUST emit or return audit metadata sufficient to record the action, decision, reason, task grant, and target refs.

#### Scenario: Gate denies email send

Given an agent requests `email.send`
When gate() denies the request
Then an audit event MUST record the denial reason
And private payloads MUST be referenced by protected refs rather than plaintext.

### Requirement: Approval-required decisions MUST not execute immediately

If gate() returns approval-required, the action MUST NOT execute until approval is recorded and validated.

#### Scenario: Draft creation requires approval

Given `email.create_draft` is approval-required
When an agent requests draft creation
Then gate() MUST return approval-required
And the connector action MUST NOT execute immediately.
