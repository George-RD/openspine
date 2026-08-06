# Tasks

## 1. Codex OAuth flow corrections

- [x] 1.1 Point `openai_codex::spec()` at `https://auth.openai.com/oauth/authorize`, mark `login_supported: true`, drop the dead device endpoint.
- [x] 1.2 Build the authorization URL with the registered redirect `http://localhost:1455/auth/callback` and the `id_token_add_organizations`, `codex_cli_simplified_flow`, `originator` parameters; refuse `begin` for `openai-codex` on any port other than 1455.
- [x] 1.3 Decode the access-token JWT claim `https://api.openai.com/auth`->`chatgpt_account_id` in `exchange_code` and `refresh_token`; fail the exchange closed when absent; carry it in `TokenResponse::account_id`.
- [x] 1.4 Remove the dead `request_device_code`/`poll_device_token` functions and their tests; replace with authorization-URL, exchange, account-id, and refresh tests (wiremock).
- [x] 1.5 Extend `setup::finish` paste parsing to accept a full redirected URL or query string (state checked when present), keeping the `code#state` form.

## 2. ChatGPT backend Responses transport

- [x] 2.1 Add `ProviderKind::OpenaiCodex` to config, with default base `https://chatgpt.com/backend-api` and a Codex fingerprint digest arm in `provider_config_digest`.
- [x] 2.2 Add `codex_fingerprint.rs` (versioned constants: originator, beta, user agent; digest fn; tests mirroring `anthropic_fingerprint`).
- [x] 2.3 Add `ProviderClient::CodexResponses` and `model_gateway/codex.rs`: request construction, SSE parsing, terminal/failure event mapping, account-id-missing error.
- [x] 2.4 Resolve the account id from the vault at call time next to the access token; verify the 401 refresh-once path covers the new variant.
- [x] 2.5 Wiremock tests: header and body shape, delta accumulation, completed-fallback extraction, `response.failed` mapping, 401-refresh retry, missing-account-id error.
- [x] 2.6 Harden the stream contract per review: CRLF framing, nested terminal-status rejection, incomplete text-or-reason semantics, secret scrubbing in error bodies.

## 3. Login enablement and switching

- [x] 3.1 `OAUTH_PROVIDER_IDS = ["anthropic", "openai-codex"]`; update the refusal comments in `setup.rs` and `oauth/providers/mod.rs`; keep Antigravity refused.
- [x] 3.2 `provider_entry` maps `openai-codex` -> `ProviderKind::OpenaiCodex`.
- [x] 3.3 `login_flow` short-circuits to verify-and-bind when a non-disabled credential with a refresh token is already stored for the named provider; the shortcut sits behind the `client_id_for` spendability refusal, backfills a missing Codex account id from the stored token, and binding cuts legacy kind/base_url entries over to the canonical transport.
- [x] 3.4 Update `setup_tests.rs`: Codex moves from the refused set to the offered set; port-exactness refusal test; Antigravity refusal stays.

## 4. Tier routing configuration

- [x] 4.1 Add optional `model_tiers` (`low`/`standard`/`high` -> provider id) to `Config` with fail-closed startup validation against the provider list.
- [x] 4.2 Build the production `GatewayTierMap` from `model_tiers` in `main.rs`; `resolve` returns the (provider id, client) pair and `api/generate.rs` threads the routed id into credential resolution (regression: `a_tier_routed_oauth_provider_spends_its_own_credential`).
- [x] 4.3 Tests: config parse, unknown-provider rejection, tier resolution end-to-end through the pool.

## 5. Examples, docs, verification (authority-sensitive: model provider access)

- [x] 5.1 `openspine.example.yaml`: Codex provider entry and `model_tiers` example; `docs/terminal-chat.md` mentions `/login openai-codex`.
- [x] 5.2 Full local gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `scripts/check-file-sizes.sh`, `npx --no-install openspec validate add-openai-codex-provider-transport --strict`, then `./scripts/check.sh`.
- [x] 5.3 Verification tasks: every scenario in the delta specs maps to a named test; `scripts/check-claims.sh` passes.
