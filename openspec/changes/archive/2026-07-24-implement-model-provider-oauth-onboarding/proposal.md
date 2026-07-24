# Proposal: Implement Model Provider OAuth Onboarding

## Dependencies

- `implement-secret-intake` (D-064..D-067) provides vault encryption (`SecretStore`) and secret staging mechanics.
- `implement-day2-operations` (AD-139, AD-144) provides first-run bootstrap posture and data directory management.
- `implement-model-swap-ceremony` (D-061..D-063) provides model role assignment (`Base`, `Fast`, `Reasoning`) and digest-bound active provider routing.
- `implement-failure-surfacing-contract` (AD-138) provides structured failure surfacing for auth/credential errors.
- Settled canon: AD-014 (secret intake), AD-144 (first-run posture), D-027 (OAuth schema pre-allocation), D-064..D-067 (vault isolation).

## Problem/Context

OpenSpine currently requires manual environment variable configuration for model provider API keys (`OPENSPINE_TEST_MISSING_PROVIDER_KEY_ENV`, `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`), and `config::ProviderAuth::Oauth` returns an error (`oauth provider login not yet implemented`). 

When users start OpenSpine to set it up as their governed personal assistant (PA), they need a streamlined onboarding experience that allows them to sign in using their existing AI model provider accounts—such as Google Antigravity, OpenAI Codex / ChatGPT, and Anthropic Claude—using native OAuth PKCE authentication rather than hunting for raw API keys.

Furthermore, OpenSpine lacks an in-tree background OAuth token refresher and a local loopback callback server/device code engine to manage token lifecycles securely without exposing unencrypted secrets or requiring manual token rotation.

## Proposed Solution

Implement native Model Provider OAuth Onboarding in OpenSpine Rust kernel (`openspine-kernel`), adding an interactive first-run setup wizard (`openspine setup` / `openspine provider login`), PKCE OAuth flow engine, encrypted vault storage, background token refresher, and gateway access token resolution.

Key components:
1. **Interactive Onboarding & Login CLI (`openspine setup`, `openspine provider login <provider>`)**:
   - Detects unconfigured model providers on first boot or when explicitly invoked.
   - Guides the user through provider selection: Google Antigravity, OpenAI Codex (ChatGPT), Anthropic Claude, or API key fallback.
   - Triggers browser-based OAuth PKCE authorization with a local loopback callback HTTP server (listening on standard provider ports: Antigravity `51121`, Codex `1455`, Anthropic `54545`, or dynamic fallback).
   - Provides seamless headless / remote SSH fallbacks: manual authorization code paste and OAuth Device Code polling flow.

2. **Encrypted Vault Credential Storage (`SecretStore`)**:
   - Stores refresh tokens, access tokens, token expiration timestamps (ms epoch), and account identity keys (`email`, `account_id`, `org_id`) in `SecretStore` encrypted with AES-256-GCM under `provider.<id>.oauth.*`.
   - Never outputs raw tokens to logs, stdout, or unencrypted telemetry.

3. **Background OAuth Refresher (`OAuthRefresher`)**:
   - Periodic timer task in `openspine-kernel` that checks OAuth token expirations every 60 seconds.
   - Preemptively refreshes tokens within 300 seconds (5 minutes) of expiration via single-flighted POST requests to provider token endpoints.
   - Distinguishes transient network errors (retried next tick) from definitive authorization failures (`invalid_grant`, revoked token), disabling invalid credentials and routing failure notifications through the `FailureSurfacingContract` (AD-138).

4. **Model Gateway Integration (`model_gateway`)**:
   - Updates `ProviderClient` to resolve active OAuth access tokens directly from `SecretStore` when `ProviderAuth::Oauth` is configured for a provider.
   - Injects `Authorization: Bearer <access_token>` into provider HTTP requests.
   - Automatically handles transient HTTP 401 responses by attempting a single forced token refresh through `SecretStore` and retrying the request once before failing.

5. **Post-Login Verification Probe & Model Role Assignment**:
   - Runs a lightweight streaming ping request ("Hello OpenSpine") against the model provider immediately after login.
   - Upon successful verification, binds the provider to active model roles (`Base`, `Fast`, `Reasoning`) and updates `openspine.yaml`.

## Acceptance Criteria

- `openspine setup` interactive CLI walks a new user from zero configuration through provider selection, OAuth login, verification, and active model role binding.
- `openspine provider login <provider>` supports Google Antigravity, OpenAI Codex, and Anthropic Claude OAuth PKCE authorization.
- Loopback callback server handles local browser redirects; headless/remote environments gracefully fall back to manual code paste and device code authentication.
- All OAuth secrets (refresh tokens, access tokens) are stored encrypted in `SecretStore` (`data_root/credentials`) and never leak into logs or unencrypted audit trails.
- The background `OAuthRefresher` automatically renews access tokens within 5 minutes of expiration without interrupting running workloads.
- Definitive refresh failures (`invalid_grant`) disable the credential and surface structured errors via the AD-138 failure surfacing queue.
- `model_gateway` seamlessly injects valid bearer access tokens into model API dispatches and recovers from transient 401 errors.
- Pre-flight verification stream calls validate provider functionality before finalizing active model provider bindings.
