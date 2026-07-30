# local-onyx-inference Specification

## Purpose
TBD - created by archiving change 2026-07-28-add-terminal-chat-onyx-lfm. Update Purpose after archive.
## Requirements
### Requirement: Onyx inference SHALL use the scoped chat API

OpenSpine SHALL support provider kind `onyx` by calling Onyx's non-streaming
`/chat/send-chat-message` API with a Personal Access Token scoped to
`write:chat`.

#### Scenario: Operator configures Onyx

- **GIVEN** Onyx has an LFM2.5 provider configured
- **AND** OpenSpine references a `write:chat` PAT from the environment
- **WHEN** the model gateway generates a response
- **THEN** it sends the request through the Onyx chat API
- **AND** the contained shell never receives the PAT.

### Requirement: OpenSpine context SHALL be ephemeral in Onyx

The adapter SHALL pass the resolved OpenSpine system and prior conversation as
`additional_context`, disable Onyx tools and citations, and send the current
user turn as the Onyx message.

#### Scenario: A terminal turn has prior history

- **WHEN** OpenSpine calls Onyx for the next turn
- **THEN** prior OpenSpine context is available to the model
- **AND** that additional context is not written into the visible Onyx message
  history.

### Requirement: LFM2.5-1.2B SHALL be the terminal default

The terminal example SHALL list `LiquidAI/LFM2.5-1.2B-Instruct` first and
register `LiquidAI/LFM2.5-350M` as an explicit smaller alternative.

#### Scenario: Operator loads the terminal example

- **WHEN** the configuration is parsed
- **THEN** both providers have kind `onyx`
- **AND** the 1.2B provider is first.

### Requirement: Onyx credentials SHALL remain outside configuration files

Provider configuration SHALL reference `ONYX_PAT` rather than storing a token
value in YAML.

#### Scenario: Terminal example is committed

- **WHEN** the example is inspected
- **THEN** it contains only the environment variable name
- **AND** no Onyx token value is present.

