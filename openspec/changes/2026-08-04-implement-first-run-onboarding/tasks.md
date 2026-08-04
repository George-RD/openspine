# Tasks

- [ ] Add `cli::readiness`: check model, injected environment lookup, provider
      credential assessment, rendering, and startup-remedy matching.
- [ ] Add `cli::onboarding`: completion marker under the data directory plus the
      first-start notice and chat help text.
- [ ] Add `cli::wizard`: interactive setup, `--check` mode, starter
      configuration and key file generation, provider login, verification, and
      role binding.
- [ ] Replace the `openspine setup` and `openspine provider login` placeholders
      in `main.rs` with dispatch into `cli::wizard`.
- [ ] Print the first-start notice and handle `/help` and `/status` in
      `run_terminal_chat`, keeping `--once` output to the reply alone.
- [ ] Wrap startup failures with their remedy in `main.rs`.

## Verification

- [ ] Test that readiness fails with a remedy for a missing configuration file,
      an unset provider environment key, and a disabled OAuth credential.
- [ ] Test that a rendered readiness report contains neither a stored access
      token nor a stored refresh token.
- [ ] Test that the starter configuration parses through `Config::load` and that
      its package directory resolves against the running executable.
- [ ] Test that role binding is skipped when verification fails and that the
      stored credential survives.
- [ ] Test that the first-start notice appears once on a ready install, every
      start on a blocked install, and never after completion is recorded.
- [ ] Test that `/help` and `/status` are handled before any event is created.
- [ ] Test that the held-lock, bound-address, missing-key, and missing-config
      failures each produce their remedy.
- [ ] Confirm the verification request reaches the provider through the model
      gateway rather than a direct HTTP call.
- [ ] Confirm no onboarding path logs or prints a token, refresh token, or API
      key.
- [ ] Run `./scripts/check.sh 2026-08-04-implement-first-run-onboarding`.
- [ ] Smoke test the built binary on the gascity NixOS host over SSH.
