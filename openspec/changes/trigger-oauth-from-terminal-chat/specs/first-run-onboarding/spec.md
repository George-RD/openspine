# first-run-onboarding Delta

## MODIFIED Requirements

### Requirement: The chat loop MUST expose help and status commands

The terminal chat loop MUST accept `/help`, `/status`, and `/login` alongside
the existing `/exit`. `/help` MUST list the available commands, name
`openspine setup`, and name `/login` as the way to start provider OAuth from
chat. `/status` MUST render the readiness assessment. `/help` and `/status`
MUST NOT create a kernel event or consume a task grant, and `/login` MUST NOT
create a kernel event or consume a task grant before the login flow starts.

#### Scenario: Owner asks for help

- **WHEN** the owner enters `/help`
- **THEN** the available commands MUST be listed
- **AND** `/login` MUST be named
- **AND** no `cli.owner.message` event MUST be created.
