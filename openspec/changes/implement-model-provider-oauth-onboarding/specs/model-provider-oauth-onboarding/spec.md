# Model Provider OAuth Onboarding Spec Delta

## ADDED Requirements

### Requirement: ProviderAuth MUST support native OAuth mode

The kernel configuration `config::ProviderAuth` MUST support `ProviderAuth::Oauth` for model providers, including Google Antigravity, OpenAI Codex, and Anthropic Claude. The kernel MUST NOT reject `ProviderAuth::Oauth` as unimplemented on startup.

#### Scenario: ProviderAuth::Oauth is valid in provider configuration

Given a `ProviderConfig` with `auth: ProviderAuth::Oauth` for `google-antigravity`
When the kernel loads the provider configuration
Then `ProviderAuth::Oauth` MUST be recognized as a valid authentication mode
And startup MUST NOT fail with `oauth provider login not yet implemented`.

Test: `config_accepts_provider_auth_oauth_variant`, `provider_config_digest_handles_oauth_providers`

---

### Requirement: Interactive setup wizard MUST execute OAuth PKCE authorization

The interactive setup CLI (`openspine setup`, `openspine provider login`) MUST initiate an OAuth 2.0 PKCE authorization flow for the selected provider. It MUST generate a cryptographically secure random code verifier, derive the `S256` code challenge, open the provider authorization URL in a local browser when available, and bind a loopback HTTP callback listener to receive the authorization code.

#### Scenario: User initiates OAuth login for Google Antigravity

Given a user selects Google Antigravity in `openspine setup`
When the OAuth PKCE flow initiates
Then the CLI MUST generate a valid S256 code challenge and state parameter
And it MUST open the Google authorization URL pointing to `127.0.0.1:51121/callback`
And it MUST receive the authorization code on the local callback listener.

Test: `oauth_pkce_generator_computes_valid_s256_challenge`, `oauth_loopback_callback_server_receives_code_and_validates_state`

---

### Requirement: OAuth login MUST support headless and remote SSH fallbacks

When local browser opening fails or an SSH/headless environment is detected, the CLI MUST provide a fallback mechanism. It MUST display the authorization URL with clear instructions, support manual authorization code paste, and support OAuth Device Authorization Code polling (`RFC 8628`) for supported providers.

#### Scenario: Headless SSH environment uses manual code input

Given an SSH environment where local browser launch is unavailable
When `openspine provider login` runs
Then the CLI MUST print the authorization URL
And it MUST prompt for manual authorization code paste
And upon input, it MUST complete token exchange successfully.

Test: `oauth_login_fallback_accepts_manual_authorization_code_input`, `oauth_device_code_flow_polls_until_token_granted`

---

### Requirement: OAuth credentials MUST be encrypted in SecretStore vault

Tokens returned from OAuth token exchange (refresh token, access token, expiration timestamp, account identity metadata) MUST be stored encrypted in `SecretStore` (`data_root/credentials`) using AES-256-GCM under key namespace `provider.<id>.oauth.*`. Raw refresh tokens and access tokens MUST NOT be stored in plaintext or logged in unencrypted telemetry.

#### Scenario: OAuth tokens are persisted into encrypted vault

Given a successful OAuth token exchange returning refresh and access tokens
When the kernel stores the credential
Then the tokens MUST be written to `SecretStore` under `provider.<id>.oauth.*`
And inspecting the raw SQLite files on disk MUST NOT reveal plaintext secret material.

Test: `oauth_tokens_stored_encrypted_in_secret_store`, `secret_store_oauth_keys_are_isolated_per_provider`

---

### Requirement: Background OAuth refresher MUST renew access tokens before expiration

The kernel MUST run a background periodic task (`OAuthRefresher`) that inspects stored OAuth credentials every 60 seconds. If an access token expires within 300 seconds (5 minutes), the refresher MUST execute a single-flighted POST request to the provider token endpoint using the refresh token, and atomically update the vault with the new access token and expiration time.

#### Scenario: Expiring access token is renewed automatically

Given a stored OAuth credential whose access token expires in 200 seconds
When the background `OAuthRefresher` executes its periodic sweep
Then it MUST issue a token refresh request to the provider
And it MUST update `provider.<id>.oauth.access_token` and `expires_at` in `SecretStore`
And concurrent refresh attempts for the same provider MUST be single-flighted.

Test: `oauth_refresher_renews_token_within_skew_window`, `oauth_refresher_single_flights_concurrent_refreshes`

---

### Requirement: Definitive OAuth refresh failures MUST surface through failure queue

If a token refresh request fails with a definitive authorization error (`invalid_grant`, `revoked_token`), the `OAuthRefresher` MUST mark the credential as disabled in `SecretStore` and emit a structured error event through the `FailureSurfacingContract` (AD-138) to notify the owner. Transient network errors MUST be retried on subsequent sweeps.

#### Scenario: Revoked refresh token disables credential and notifies owner

Given a refresh token that has been revoked by the provider
When `OAuthRefresher` attempts to renew the token and receives `HTTP 400 invalid_grant`
Then the credential MUST be marked disabled in `SecretStore`
And a structured failure notification MUST be enqueued in the owner failure queue.

Test: `oauth_refresher_handles_definitive_failure_and_enqueues_notification`, `oauth_refresher_retains_credential_on_transient_network_failure`

---

### Requirement: Model gateway MUST resolve live OAuth access tokens from vault

When `model_gateway` dispatches a model request for a provider configured with `ProviderAuth::Oauth`, `ProviderClient` MUST resolve the active access token from `SecretStore`, inject `Authorization: Bearer <access_token>` into the request headers, and retry ONCE on HTTP 401 Unauthorized after triggering an inline token refresh.

#### Scenario: Gateway dispatches model request using OAuth bearer token

Given a `model.generate` request routed to an OAuth-authenticated provider
When `ProviderClient` constructs the outbound HTTP request
Then it MUST retrieve the valid access token from `SecretStore`
And it MUST attach `Authorization: Bearer <access_token>` to the request headers.

Test: `gateway_injects_oauth_bearer_token_from_vault`, `gateway_recovers_from_transient_401_via_inline_token_refresh`

---

### Requirement: Setup wizard MUST verify provider connectivity before activating roles

After completing OAuth login, the setup wizard MUST execute a lightweight pre-flight streaming ping request against the provider. Only after successful ping verification MAY the wizard update `openspine.yaml` to bind the provider to active model roles (`Base`, `Fast`, `Reasoning`).

#### Scenario: Pre-flight ping verifies provider before role binding

Given a successful OAuth login for Anthropic Claude
When the setup wizard runs the verification probe
Then it MUST send a lightweight stream ping request through `model_gateway`
And upon receiving a valid completion, it MUST set active model provider roles to `anthropic`.

Test: `setup_wizard_runs_preflight_verification_ping`, `setup_wizard_binds_active_model_roles_only_on_successful_verification`
