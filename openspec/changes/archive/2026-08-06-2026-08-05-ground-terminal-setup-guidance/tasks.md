# Tasks

- [x] Bump `owner_terminal_template` to version 2 with setup-surface
      grounding and the credential refusal rule.
- [x] Add a loader contract test pinning the shipped template's setup
      guidance.

## Verification

- [x] Test that the loaded package's `owner_terminal_template` names the CLI
      setup commands and the in-chat `/status` command, and forbids
      credential intake in conversation.
- [x] Run `./scripts/check.sh 2026-08-05-ground-terminal-setup-guidance`.
