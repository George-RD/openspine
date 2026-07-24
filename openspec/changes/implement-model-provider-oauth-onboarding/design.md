# Design: Model Provider OAuth Onboarding

## Overview & Architecture

Model Provider OAuth Onboarding introduces native OAuth 2.0 PKCE authentication, encrypted credential management, background token lifecycle maintenance, and an interactive onboarding CLI wizard to OpenSpine (`openspine-kernel`).

```
+-----------------------------------------------------------------------------------+
| USER TERMINAL / BROWSER                                                           |
|  openspine setup  --->  Browser OAuth Consent ---> Loopback Callback / Device Code|
+------------------------------------------+----------------------------------------+
                                           |
                                           v
+-----------------------------------------------------------------------------------+
| OPENSPINE KERNEL                                                                  |
|                                                                                   |
|  +---------------------------+       +-----------------------------------------+  |
|  | Onboarding & Login CLI    |       | OAuth PKCE Engine                       |  |
|  | - Provider Selection      | ----> | - Code Challenge (S256) & Verifier      |  |
|  | - Model Role Assignment   |       | - Loopback Server (51121, 1455, 54545)  |  |
|  +---------------------------+       | - Device Code / Manual Code Fallback    |  |
|                                      +--------------------+--------------------+  |
|                                                           |                       |
|                                                           v                       |
|  +---------------------------+       +--------------------+--------------------+  |
|  | Background Refresher      | <---> | Encrypted Vault (SecretStore)           |  |
|  | - 60s Periodic Sweep      |       | - AES-256-GCM (data_root/credentials)   |  |
|  | - Preemptive 300s Refresh |       | - provider.<id>.oauth.* keys            |  |
|  +---------------------------+       +--------------------+--------------------+  |
|                                                           |                       |
|                                                           v                       |
|                                      +--------------------+--------------------+  |
|                                      | Model Gateway (ProviderClient)          |  |
|                                      | - Bearer Token Resolution               |  |
|                                      | - Single-retry on HTTP 401              |  |
|                                      +--------------------+--------------------+  |
+-----------------------------------------------------------+-----------------------+
                                                            |
                                                            v
+-----------------------------------------------------------------------------------+
| AI MODEL PROVIDER APIS                                                            |
|  Google Antigravity | OpenAI Codex / ChatGPT | Anthropic Claude                    |
+-----------------------------------------------------------------------------------+
```

---

## OAuth Provider Specifications

The native OAuth engine supports three pre-configured provider integrations plus custom OAuth provider registrations:

| Provider ID | Display Name | Auth Endpoint | Token Endpoint | Scope / Audience | Default Port | Fallback Mode |
| ----------- | ------------ | ------------- | -------------- | ---------------- | ------------ | ------------- |
| `google-antigravity` | Google Antigravity | `https://accounts.google.com/o/oauth2/v2/auth` | `https://oauth2.googleapis.com/token` | `https://www.googleapis.com/auth/cloud-platform email` | `51121` | Loopback / Manual Code |
| `openai-codex` | OpenAI Codex / ChatGPT | `https://auth.openai.com/authorize` | `https://auth.openai.com/oauth/token` | `openid profile email offline_access` | `1455` | Loopback / Device Code |
| `anthropic` | Anthropic Claude | `https://claude.ai/oauth/authorize` | `https://api.anthropic.com/v1/oauth/token` | `org:read user:read` | `54545` | Loopback / Manual Code |

---

## Encrypted Vault Storage Schema (`SecretStore`)

All OAuth tokens and metadata are stored in `SecretStore` (`data_root/credentials`) encrypted using master key derived AES-256-GCM.

Keys for a given provider (e.g. `google-antigravity`):

- `provider.<id>.auth_mode`: `"oauth"`
- `provider.<id>.refresh_token`: Encrypted string (refresh token)
- `provider.<id>.access_token`: Encrypted string (current access token)
- `provider.<id>.expires_at`: Encrypted string (ISO-8601 timestamp / epoch ms when `access_token` expires)
- `provider.<id>.account_email`: Encrypted string (user email address, if provided by token exchange)
- `provider.<id>.account_id`: Encrypted string (provider account / org ID)
- `provider.<id>.identity_key`: Encrypted string (composite identity e.g. `email:user@example.com|org:org-123`)

No unencrypted tokens or plaintext auth codes are ever stored on disk or emitted in logs.

---

## OAuth PKCE & Callback Engine (`openspine-kernel::oauth`)

### PKCE Generator
- Generates a cryptographically secure 32-byte random code verifier (`URL-safe base64`).
- Computes `SHA-256(verifier)` and base64url encodes it as `code_challenge` (`S256` method).

### Loopback Callback HTTP Server
- Spawns a temporary `tokio` TCP listener on `127.0.0.1:<preferred_port>` (falling back to port `0` for dynamic binding if preferred port is occupied).
- Listens for GET request to `/callback` (or `/`).
- Validates the returned `state` parameter against the generated CSRF state token.
- Returns a friendly HTML HTML success page: `"Authentication successful! You can close this window and return to OpenSpine."`
- Has a strict 3-minute timeout (`Duration::from_secs(180)`).

### Headless / Remote SSH Fallbacks
1. **Manual Code Paste**: If opening local browser fails or is declined by user (e.g. running over SSH), the CLI prints the full Auth URL and prompts: `"Enter authorization code (or press Enter for device code):"`.
2. **Device Code Flow**: For OpenAI Codex, supports standard OAuth 2.0 Device Authorization Grant (`RFC 8628`), printing user code & verification URL and polling token endpoint until completed.

---

## Background Token Refresher (`OAuthRefresher`)

The `OAuthRefresher` operates as a background task within `openspine-kernel`:

1. **Periodic Sweep**: Ticks every 60 seconds (`tokio::time::interval`).
2. **Expiry Check**: Scans all active `ProviderConfig` entries where `auth` is `ProviderAuth::Oauth`.
3. **Preemptive Refresh Window**: If `expires_at - now < 300 seconds` (5 minutes), the refresher triggers a token renewal.
4. **Single-Flighting**: Prevents concurrent refresh attempts for the same provider ID using `parking_lot::Mutex` or `tokio::sync::Mutex` locks per provider slot.
5. **Token Exchange**: Sends `grant_type=refresh_token` POST request with `refresh_token` and client parameters.
6. **Atomic Vault Update**: On success, updates `access_token`, `expires_at`, and (if rotated) `refresh_token` in `SecretStore` within a single atomic operation.
7. **Failure Classification**:
   - *Transient Errors* (network timeout, HTTP 5xx): Logged as warning; retained for retry on next tick.
   - *Definitive Errors* (HTTP 400/401 with `invalid_grant`, `revoked_token`): Credential marked disabled in vault, and a failure event is emitted via `FailureSurfacingContract` (AD-138) to notify the owner.

---

## Model Gateway Integration (`model_gateway`)

In `crates/openspine-kernel/src/model_gateway/mod.rs`:

1. When dispatching a request for a `ProviderConfig` with `ProviderAuth::Oauth`:
   - `ProviderClient` fetches `provider.<id>.access_token` from `SecretStore`.
   - If `access_token` is missing or expired, triggers an inline synchronous refresh via `OAuthRefresher`.
   - Attaches `Authorization: Bearer <access_token>` header to the outbound HTTP request.
2. **Transient 401 Handling**:
   - If the provider API responds with `HTTP 401 Unauthorized`:
     - Forces an immediate token refresh through `OAuthRefresher`.
     - Retries the model request ONCE with the new access token.
     - If the retry also fails with 401, surface a structured `ProviderAuthError` to the caller.

---

## Interactive Setup Wizard (`openspine setup`)

The interactive setup wizard provides a friendly terminal walkthrough:

```
======================================================
  Welcome to OpenSpine - Governed AI Personal Assistant
======================================================

Step 1: AI Model Provider Setup
------------------------------------------------------
Select your primary AI model provider:
  [1] Google Antigravity (OAuth Login) - Recommended
  [2] OpenAI Codex / ChatGPT (OAuth Login)
  [3] Anthropic Claude (OAuth Login)
  [4] Custom API Key (Anthropic / OpenAI-compatible)

Choice [1]: 1

Initiating OAuth login for Google Antigravity...
Opening browser: https://accounts.google.com/o/oauth2/v2/auth?...
Waiting for browser authentication on port 51121...

[✓] Authentication successful!
[✓] Received refresh token and access token.
[✓] Stored encrypted credentials in vault.

Step 2: Verification Ping
------------------------------------------------------
Testing model provider connectivity (google-antigravity / gemini-2.5-flash)...
[✓] Response received: "Hello! OpenSpine model gateway verified."

Step 3: Role Assignment
------------------------------------------------------
Assigning google-antigravity to model roles:
  - Base Model Role       : google-antigravity
  - Fast Model Role       : google-antigravity
  - Reasoning Model Role  : google-antigravity

[✓] Updated openspine.yaml
[✓] Setup complete! OpenSpine kernel is ready.
```
