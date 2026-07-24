# Tasks: Model Provider OAuth Onboarding

## Phase 1: Configuration & Schema Foundation

- [x] 1.1 Update `ProviderAuth` in `crates/openspine-kernel/src/config.rs`:
  - Support `ProviderAuth::Oauth` without returning `oauth provider login not yet implemented` error.
  - Implement helper functions to resolve provider OAuth keys and metadata from configuration.
  - Add unit tests in `config.rs` testing `ProviderAuth::Oauth` parsing and provider digest calculation.

- [x] 1.2 Extend Secret Store (`SecretStore`) helper methods in `crates/openspine-kernel/src/secret_store.rs`:
  - Add `store_oauth_tokens(provider_id, refresh_token, access_token, expires_at, identity_metadata)`.
  - Add `get_oauth_tokens(provider_id)` and `update_access_token(provider_id, access_token, expires_at)`.
  - Add `disable_oauth_credential(provider_id, cause)`.
  - Add unit tests in `secret_store.rs` proving secret isolation and encrypted persistence.

## Phase 2: OAuth PKCE Engine & Flow Mechanics

- [x] 2.1 Implement PKCE generator in `crates/openspine-kernel/src/oauth/pkce.rs`:
  - Generate 32-byte cryptographically secure code verifier.
  - Compute SHA-256 code challenge with `S256` method.
  - Unit tests verifying challenge calculation against standard test vectors.

- [x] 2.2 Implement Loopback Callback HTTP Server in `crates/openspine-kernel/src/oauth/callback_server.rs`:
  - Spawn `tokio` TCP listener on preferred port (e.g. 51121, 1455, 54545) with fallback to port 0.
  - Handle `/callback` GET requests, validate CSRF `state` parameter, and return HTML success response.
  - Enforce 3-minute timeout (`Duration::from_secs(180)`).
  - Integration tests using `reqwest` or `hyper` simulating browser OAuth callback.

- [x] 2.3 Implement Provider Specs & Flow Engine in `crates/openspine-kernel/src/oauth/providers/`:
  - Implement `google_antigravity.rs`: Auth URL, token URL, Google scopes, client ID, token exchange.
  - Implement `openai_codex.rs`: Auth URL, token URL, Codex client ID, PKCE exchange, device code polling engine.
  - Implement `anthropic.rs`: Auth URL, token URL, Claude client ID, PKCE exchange, manual code paste fallback.
  - Integration tests for each provider flow using mock HTTP token servers.

## Phase 3: Background Refresher & Gateway Integration

- [x] 3.1 Implement `OAuthRefresher` background task in `crates/openspine-kernel/src/oauth/refresher.rs`:
  - 60-second periodic sweep scanning active `ProviderAuth::Oauth` providers.
  - Check expiration (`expires_at - now < 300 seconds`).
  - Perform single-flighted refresh POST requests using `SecretStore` refresh tokens.
  - Atomically update `SecretStore` on success.
  - Handle transient errors (retry next tick) vs. definitive failures (`invalid_grant` -> disable credential + enqueue failure event via `FailureSurfacingContract` AD-138).
  - Unit/integration tests simulating expiring tokens, single-flighting, and failure surfacing.

- [x] 3.2 Wire Model Gateway (`model_gateway`) to OAuth Vault:
  - Update `ProviderClient::dispatch` in `crates/openspine-kernel/src/model_gateway/mod.rs` to fetch active access token from `SecretStore` when `ProviderAuth::Oauth` is set.
  - Inject `Authorization: Bearer <access_token>` into outbound provider HTTP requests.
  - Implement HTTP 401 recovery: force inline token refresh and retry request once before raising `ProviderAuthError`.
  - Integration tests verifying token injection and 401 retry recovery.

## Phase 4: Interactive Onboarding Setup Wizard & CLI

- [x] 4.1 Implement `openspine setup` & `openspine provider login` CLI in `crates/openspine-kernel/src/cli/setup.rs` & `main.rs`:
  - Build interactive terminal wizard prompting user for provider selection.
  - Initiate browser opening / loopback callback or headless fallback (manual code paste / device code).
  - Perform token exchange and write credentials to `SecretStore`.
  - Execute pre-flight streaming ping request against model provider to verify connectivity.
  - Bind active model roles (`Base`, `Fast`, `Reasoning`) and update `openspine.yaml`.
  - End-to-end integration tests exercising `openspine setup` CLI flow in test mode.

## Phase 5: Verification & Quality Gates

- [x] 5.1 Run full local quality gates:
  - `./scripts/check.sh` (cargo fmt, cargo clippy --workspace --all-targets -- -D warnings, cargo test --workspace, file-size gate).
  - `openspec validate --all --strict` (or strict delta spec validation).
