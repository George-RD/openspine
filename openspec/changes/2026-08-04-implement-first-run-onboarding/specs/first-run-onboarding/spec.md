# First-run onboarding

## ADDED Requirements

### Requirement: Readiness assessment MUST name every blocking gap and its remedy

The kernel MUST expose a deterministic readiness assessment covering the
configuration file, the required key material, the configured model providers,
and each provider's credential state. Every check MUST report a pass, warn, or
fail state, and every failing check MUST carry a remedy naming the command,
environment variable, or file that resolves it. The assessment MUST NOT include
secret material in any rendered output.

#### Scenario: Unconfigured install is assessed

- **WHEN** readiness is assessed with no configuration file present
- **THEN** the configuration check MUST fail
- **AND** its remedy MUST name `openspine setup`
- **AND** the assessment MUST report the install as not ready.

#### Scenario: Provider expects an unset environment key

- **GIVEN** a provider configured with `auth.mode: api_key` and `env: SOME_KEY`
- **WHEN** readiness is assessed and `SOME_KEY` is unset
- **THEN** the provider credential check MUST fail
- **AND** the remedy MUST name `SOME_KEY`.

#### Scenario: OAuth provider credential is disabled

- **GIVEN** a provider configured with `auth.mode: oauth`
- **AND** a stored vault credential marked disabled
- **WHEN** readiness is assessed
- **THEN** the provider credential check MUST fail
- **AND** the remedy MUST name `openspine provider login` for that provider id.

#### Scenario: Rendered output excludes secret material

- **GIVEN** a stored OAuth access token and refresh token
- **WHEN** the readiness report is rendered
- **THEN** the rendered text MUST NOT contain either token value.

### Requirement: Key material MUST load from an owner-only file beside the configuration

Every `openspine` invocation MUST load environment entries from an
`openspine.env` file adjacent to the resolved configuration file before it reads
key material. An entry already present in the process environment MUST take
precedence over the file, so an operator-supplied environment is never
overridden. The file MUST be rejected when it is readable by any account other
than its owner, and the failure MUST name the file and the required mode.

Without this, a generated key file would be inert: `OPENSPINE_ARTIFACT_KEY`,
`OPENSPINE_GRANT_HMAC_KEY`, and `OPENSPINE_WEBHOOK_HMAC_KEY` are read only from
the process environment.

#### Scenario: Generated key file makes the install runnable

- **GIVEN** a wizard-generated `openspine.env` beside the configuration file
- **AND** none of its variables are set in the process environment
- **WHEN** any `openspine` command starts
- **THEN** those variables MUST be readable through the process environment
- **AND** startup MUST NOT fail for absent key material.

#### Scenario: Process environment wins over the file

- **GIVEN** `OPENSPINE_ARTIFACT_KEY` is already set in the process environment
- **AND** the adjacent file sets a different value
- **WHEN** the file is loaded
- **THEN** the process environment value MUST be retained.

#### Scenario: Group-readable key file is refused

- **GIVEN** an adjacent key file whose mode grants group or world read
- **WHEN** the file is loaded
- **THEN** loading MUST fail
- **AND** the failure MUST name the file path and the required owner-only mode.

### Requirement: `openspine setup` MUST perform real onboarding work

`openspine setup` MUST NOT be a placeholder. It MUST report the resolved
configuration and data paths, render the readiness assessment, and offer
provider login, re-assessment, and provider verification. When the configuration
file is absent, it MUST be able to write a starter configuration and a key file
containing freshly generated key material.

The starter configuration's package directory MUST be resolved from the running
executable rather than from the process working directory, so an installed
binary does not depend on where it was invoked.

#### Scenario: Setup writes a starter configuration

- **GIVEN** no configuration file exists at the resolved path
- **WHEN** the owner accepts the starter configuration
- **THEN** a parseable configuration MUST be written at that path
- **AND** a key file containing artifact, grant, and webhook key material MUST
  be written with owner-only permissions
- **AND** the configuration's package directory MUST resolve against the running
  executable.

#### Scenario: Setup reports readiness without prompting

- **WHEN** `openspine setup --check` runs
- **THEN** the readiness report MUST be printed without reading standard input
- **AND** the process MUST exit non-zero when any check fails
- **AND** the process MUST exit zero when every check passes.

#### Scenario: Setup refuses to write vault state under a running kernel

- **GIVEN** a running kernel holds the data-root lifetime lock
- **WHEN** `openspine setup` runs
- **THEN** it MUST report that an instance is already running
- **AND** it MUST NOT open a second view of the credential vault.

### Requirement: Provider login MUST refuse before offering an unregistered client

Provider OAuth login MUST resolve the provider's registered OAuth client id from
its environment variable and MUST refuse before producing an authorization URL
when that variable is unset. There MUST NOT be a built-in client id default.

OpenSpine cannot register an OAuth application on the owner's behalf. Printing an
authorization URL that the provider rejects on arrival reproduces the failure
this change exists to remove, so the refusal MUST name the variable, the redirect
URI the registration needs, and the API-key alternative.

#### Scenario: Owner logs in to a provider with no registered client

- **GIVEN** `OPENSPINE_ANTHROPIC_CLIENT_ID` is unset
- **WHEN** the owner runs `openspine provider login anthropic`
- **THEN** the command MUST fail before any authorization URL is printed
- **AND** the failure MUST name `OPENSPINE_ANTHROPIC_CLIENT_ID`
- **AND** it MUST name the loopback redirect URI the registration requires
- **AND** it MUST name configuring an API-key or local provider instead.

#### Scenario: Token exchange presents the authorizing client

- **GIVEN** an authorization begun with a resolved client id
- **WHEN** the code is exchanged for tokens
- **THEN** the exchange MUST present the same client id the authorization URL
  carried.

### Requirement: Model roles MUST be bound only after successful verification

The wizard MUST send a verification request through the model gateway after a
provider login and MUST update the provider's auth mode in `openspine.yaml`
only when that request succeeds. A failed verification MUST leave the stored
credential and the configuration file unchanged.

#### Scenario: Verification fails after login

- **GIVEN** a completed provider login that stored a credential
- **WHEN** the verification request fails
- **THEN** `openspine.yaml` MUST NOT be modified
- **AND** the stored credential MUST remain so a retry does not repeat the
  authorization.

### Requirement: Terminal chat MUST recognize an unconfigured first start

`openspine chat` MUST assess readiness before its first prompt. When the install
is not ready, it MUST print the blocking checks and their remedies on every
start. When the install is ready and onboarding has not been recorded as
complete, it MUST print an orientation notice once and then record completion.
When the install is ready and onboarding is recorded as complete, it MUST print
no notice.

Completion state MUST be stored as runtime state under the data directory and
MUST NOT be written into the owner's configuration file.

#### Scenario: First start on an unconfigured install

- **GIVEN** onboarding has never completed and a provider credential is missing
- **WHEN** the owner starts `openspine chat`
- **THEN** the blocking checks MUST be printed with their remedies
- **AND** completion MUST NOT be recorded.

#### Scenario: First start on a ready install

- **GIVEN** every readiness check passes and no completion is recorded
- **WHEN** the owner starts `openspine chat`
- **THEN** an orientation notice MUST be printed
- **AND** completion MUST be recorded so a later start prints no notice.

#### Scenario: One-shot chat stays machine readable

- **WHEN** the owner runs `openspine chat --once "hello"`
- **THEN** standard output MUST contain only the governed reply.

### Requirement: The chat loop MUST expose help and status commands

The terminal chat loop MUST accept `/help` and `/status` alongside the existing
`/exit`. `/help` MUST list the available commands and name `openspine setup`.
`/status` MUST render the readiness assessment. Neither command MUST create a
kernel event or consume a task grant.

#### Scenario: Owner asks for help

- **WHEN** the owner enters `/help`
- **THEN** the available commands MUST be listed
- **AND** no `cli.owner.message` event MUST be created.

### Requirement: Known startup failures MUST name their remedy

When startup fails for a cause with a known remedy, the kernel MUST print that
remedy alongside the error. Covered causes MUST include a held data-root
lifetime lock, an already-bound listener address, absent required key material,
and an absent configuration file.

#### Scenario: A startup failure shows every blocking check

- **GIVEN** neither a configuration file nor key material exists
- **WHEN** any command fails at startup
- **THEN** the printed report MUST list every blocking check, not only the
  remedy for the first failure
- **AND** it MUST name `openspine setup --check`.

#### Scenario: A second instance starts

- **GIVEN** a running instance holds the data-root lifetime lock
- **WHEN** a second instance starts
- **THEN** the printed failure MUST state that an instance is already running
- **AND** it MUST name stopping that instance as the remedy.

#### Scenario: The listener address is already bound

- **GIVEN** another process holds the configured bind address
- **WHEN** startup binds the listener
- **THEN** the printed failure MUST name the bind address
- **AND** it MUST name changing `kernel.bind_addr` as the remedy.
