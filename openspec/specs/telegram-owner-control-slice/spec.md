# Spec: Telegram owner control slice

## Purpose

Implement the minimum usable owner-control lane: a Telegram connector that verifies the configured owner's Telegram ID, routes verified owner messages through deterministic routing and authority composition into a gate-mediated task grant, so every effect the assistant takes is authorized and audited.

## Requirements

### Requirement: Telegram owner messages MUST be source verified

Telegram owner-control events MUST verify the configured owner Telegram user ID before routing to owner control.

#### Scenario: Configured owner sends message

Given a Telegram update is received from the configured owner user ID
When the event is normalized
Then `verified_source` MUST be true
And `verification_method` MUST indicate owner Telegram ID match.

#### Scenario: Unknown Telegram user sends message

Given a Telegram update is received from any other Telegram user ID
When the event is normalized
Then the event MUST NOT receive owner-control authority.

### Requirement: Owner Telegram messages MUST normalize into event envelopes

Verified owner Telegram messages MUST normalize into `telegram.owner.message` event envelopes.

#### Scenario: Owner message is normalized

Given a verified owner Telegram message
When the event envelope is created
Then the event type MUST be `telegram.owner.message`
And the lane MUST be `owner_control`.

### Requirement: Telegram owner route MUST resolve deterministically

The Telegram owner route MUST deterministically select `main_assistant_agent`, `owner_control_conversation`, and `owner_control_basic_pack`.

#### Scenario: Owner control route matches

Given a verified `telegram.owner.message` event
When route resolution runs
Then it MUST select the owner-control route
And ambiguous route conflicts MUST fall back to low-authority review.

### Requirement: Main assistant task grant MUST be narrow

The main assistant task grant MUST allow only owner-control actions needed for status, setup guidance, approved workflow invocation, artifact proposal, model-mediated reply, and Telegram owner reply.

#### Scenario: Main assistant requests email inbox read

Given the main assistant receives an owner-control task grant
When it requests `email.read_inbox`
Then gate() MUST deny the request.

### Requirement: Telegram reply MUST use the owner channel only

Owner-control replies MUST be sent only to the verified owner channel associated with the task grant.

#### Scenario: Agent attempts reply to different chat

Given the owner-control task grant is bound to one Telegram chat
When the agent requests a reply to another chat
Then gate() MUST deny the request.
