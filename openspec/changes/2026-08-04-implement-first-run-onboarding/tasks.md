# Tasks

- [x] Add `cli::readiness`: check model, injected environment lookup, provider
      credential assessment, rendering, and startup-remedy matching.
- [x] Add `cli::onboarding`: completion marker under the data directory plus the
      first-start notice and chat help text.
- [x] Add `cli::wizard`, `cli::login`, `cli::starter`, `cli::prompt`:
      interactive setup, `--check` mode, starter configuration and key file
      generation, provider login, verification, and role binding.
- [x] Add `env_file`: load key material from an owner-only `openspine.env`
      beside the configuration, with the process environment taking precedence.
- [x] Replace the `openspine setup` and `openspine provider login` placeholders
      in `main.rs` with dispatch into the wizard.
- [x] Print the first-start notice and handle `/help` and `/status` in
      `run_terminal_chat`, keeping `--once` output to the reply alone.
- [x] Wrap startup failures with their remedy in `main.rs`.
- [x] Promote a verified OAuth provider to `providers[0]`, since
      `select_default_provider_id` routes to the first entry.
- [x] Resolve each provider's OAuth client id from its environment variable and
      refuse before producing an authorization URL when it is unset, replacing
      the hardcoded placeholder client ids.
- [x] Render the blocking checklist, not only one remedy, when startup fails.

## Verification

- [x] Test that readiness fails with a remedy for a missing configuration file,
      an unset provider environment key, and a disabled OAuth credential.
- [x] Test that a rendered readiness report contains neither a stored access
      token nor a stored refresh token, and that a probe failure redacts the
      vault tokens and a short API key from the quoted provider body.
- [x] Test that the starter configuration parses through `Config::load`, that
      its package directory resolves against the running executable, and that
      prompt-supplied values cannot corrupt or inject YAML.
- [x] Test that role binding is skipped when verification fails and that the
      stored credential survives.
- [x] Test that a re-login without a reissued refresh token keeps the stored
      one, and that a first login without one is refused rather than storing a
      placeholder.
- [x] Test that the first-start notice appears once on a ready install, every
      start on a blocked install, and never after completion is recorded.
- [x] Test that the generated key file is created at mode 0600 without ever
      writing key material into a pre-existing world-readable inode.
- [x] Test that an environment-supplied key is carried into the generated file
      unchanged, and that a populated data root blocks minting a new artifact
      key.
- [x] Test that the held-lock, bound-address, missing-key, and missing-config
      failures each produce their remedy.
- [x] Confirm the verification request reaches the provider through the model
      gateway rather than a direct HTTP call.
- [x] Run `./scripts/check.sh 2026-08-04-implement-first-run-onboarding`.
- [x] Test that a provider with no registered client is refused before a URL
      exists, and that the token exchange presents the authorizing client id.
- [x] Test that a startup failure lists every blocking check.
- [x] Smoke test the built binary on the gascity NixOS host over SSH: fresh
      install, interactive bootstrap, first and second chat start, one-shot
      reply, a configuration with no key file, and headless provider login.
