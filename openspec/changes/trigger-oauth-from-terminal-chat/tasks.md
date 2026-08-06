# Tasks

## 1. Local command surface

- [x] 1.1 Add a pure local-command parser in `cli::onboarding` covering
      `/exit`, `/quit`, `/help`, `/status`, `/login`, and `/login <provider>`,
      with unit tests for defaulting, whitespace, and non-commands.
- [x] 1.2 Name `/login` in `help_text()` and in the Lyra terminal template's
      setup guidance, keeping the existing contract-test needles passing.

## 2. Chat teardown handoff

- [x] 2.1 Make `run_terminal_chat` return the requested login provider after
      stopping the chat HTTP server; `--once` keeps treating `/login` as an
      ordinary governed message.
- [x] 2.2 In `run`, drop the remaining `secrets` and `overlay_operations`
      owners after chat returns, then dispatch `cli::login::run_provider_login`
      and exit with its outcome.

## 3. Verification

- [x] 3.1 Test that the teardown order releases the data-root lifetime lock: a
      fresh `overlay_export_restore::acquire` succeeds after the chat state and
      local owners drop, and fails while they are held.
- [x] 3.2 Test that `/login` resolves locally with no kernel event or task
      grant, and that `/help` names `/login`.
- [x] 3.3 Run `./scripts/check.sh trigger-oauth-from-terminal-chat`.
