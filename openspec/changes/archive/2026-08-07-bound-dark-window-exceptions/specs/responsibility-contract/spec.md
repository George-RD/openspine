# Responsibility contract

## MODIFIED Requirements

### Requirement: Communication dark-window Allow MUST be forbidden

Reusable delegation for communication or connector-write effects MUST reject any policy that permits a dark-window Allow default. Timeout behavior MUST remain deny or require explicit review until a future decision changes this posture.

Enforcement MUST exist on the activation path, not only in review. Dark-window Allow eligibility MUST be an explicit catalog allowlist: an action is eligible only when it is named on that allowlist, and an action the catalog does not know MUST be treated as ineligible. Eligibility MUST NOT be inferred from other declarations, because a predicate that permits whatever it cannot classify is not fail closed. The kernel MUST refuse to activate a standing rule whose dark-window default is Allow for an ineligible action, before the rule is persisted as active, leaving no active rule row and durable owner-actionable evidence naming the action. Because activation is not retroactive, the kernel MUST also converge stored state: an active rule whose stored Allow default is ineligible MUST be moved out of live consultation, with its unresolved pending exceptions staled in the same transaction. Permitting an action MUST be an explicit catalog decision with proposal-specific proof.

#### Scenario: Draft action declares bounded Allow

- **WHEN** a communication or owner-account write descriptor declares a bounded Allow dark window
- **THEN** delegation validation MUST fail before owner review

#### Scenario: Activation refuses an Allow default for an ineligible action

- **WHEN** a standing rule whose dark-window default is Allow is activated for an action the catalog does not name on the eligibility allowlist
- **THEN** activation MUST be refused
- **AND** no active rule row MUST be written
- **AND** durable evidence MUST record the refusal and the action it applied to

#### Scenario: A stored ineligible Allow rule is retired on open

- **WHEN** the kernel opens a database holding an active rule whose dark-window default is Allow for an ineligible action
- **THEN** that rule MUST be moved out of live consultation
- **AND** its unresolved pending exceptions MUST be staled
- **AND** durable evidence MUST record the retirement

#### Scenario: A Deny default stays permitted

- **WHEN** the same rule declares a Deny dark-window default instead
- **THEN** activation MUST succeed
- **AND** the timeout behaviour MUST remain deny
