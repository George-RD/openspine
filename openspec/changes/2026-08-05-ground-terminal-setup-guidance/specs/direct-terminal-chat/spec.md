# direct-terminal-chat Delta

## ADDED Requirements

### Requirement: The terminal assistant SHALL route setup to the CLI and refuse credential intake

The terminal assistant's prompt template SHALL ground the assistant in the
CLI setup surface: `openspine setup`, `openspine setup --check`,
`openspine provider login <provider>`, and the in-chat `/status` and `/help`
commands. The template SHALL instruct the assistant never to request or
accept API keys, client ids, secrets, or tokens in conversation, and to
treat a pasted secret as exposed, because chat text enters model context
(D-014).

#### Scenario: Shipped template grounds the setup surface

- **WHEN** the kernel loads the shipped `artifacts/lyra` package
- **THEN** the `owner_terminal_template` system preamble names
  `openspine provider login`, `openspine setup --check`, `/status`, and
  `/help`
- **AND** instructs the assistant never to request or accept credentials in
  conversation.
