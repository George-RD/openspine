## Why

The owner asked Lyra to set up OAuth and expected the terminal UX to launch the real flow. Prompt guidance can point at the CLI, but a small local model can garble the command and must never be trusted to trigger account authorization. OpenSpine needs an explicit, kernel-local owner gesture that starts the existing OAuth PKCE flow without putting credentials or authorization codes into model context.

## What Changes

- Add `/login` and `/login <provider>` to direct terminal chat as local commands that create no event, grant, model call, or conversation-history row.
- On `/login`, stop the chat HTTP server, release the live runtime and data-root lifetime lock, then invoke the existing `openspine provider login` path in the same terminal.
- Exit after the login attempt. A successful verified login rewrites the provider binding; the next `openspine` start loads that configuration.
- Add deterministic parsing and lock-release regression tests.
- Update `/help` and the Lyra terminal prompt to name the local `/login` gesture.

This affects OpenSpine core's local CLI lifecycle and Lyra's owner-facing setup UX. It changes no runtime authority, connector access, or private-data reach. External communication occurs only through the existing OAuth flow after the owner explicitly enters `/login`. The authorization code and tokens continue to bypass the shell/model path and enter the encrypted vault through the existing setup implementation.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `direct-terminal-chat`: adds a local OAuth login command and requires chat teardown before credential-vault login starts.
- `first-run-onboarding`: extends local help with the explicit login gesture while preserving the no-event/no-grant contract.

## Impact

- `crates/openspine-kernel/src/main.rs`: local command dispatch, chat outcome, and ordered resource teardown.
- `crates/openspine-kernel/src/cli/onboarding.rs`: command parser/help text and focused tests.
- `artifacts/lyra/templates/owner_terminal_template.yaml`: prompt grounding for `/login`.
- Existing `cli::login::run_provider_login` remains the only OAuth entry point after chat teardown.
- No new dependencies.

Non-goals: triggering OAuth from natural-language model output; logging in while the chat HTTP server is live; hot-reloading the immutable provider pool; adding provider transports; changing the existing PKCE, verification, vault, or config-binding contract; automatically authorizing an account without owner browser interaction.
