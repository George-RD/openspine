# Add OpenAI Codex OAuth provider transport and tier routing

## Dependencies

- `implement-model-provider-oauth-onboarding` (archived): PKCE flow, vault storage, refresher, preflight verification.
- `implement-model-swap-ceremony` (archived): provider config digests bind swap approvals.
- `2026-07-28-add-terminal-chat-onyx-lfm` (archived): provider pool and tier map plumbing.

## Problem/Context

The build ships a working Codex authorization flow but refuses the login (`login_supported: false`) because the gateway cannot spend the credential: Codex OAuth tokens are only accepted by `chatgpt.com/backend-api/codex/responses` with a `chatgpt-account-id` header, a transport the gateway does not implement. The refusal was correct (D-145 posture: never store a credential no request can use), but it leaves the owner with exactly one hosted login. The owner wants Codex as a second subscription login, the ability to switch the bound provider when both credentials exist, and different models for different task tiers.

Three concrete defects block the login even at the authorization step, verified against the first-party Codex client contract and pi's working implementation:

1. Wrong authorize endpoint: `https://auth.openai.com/authorize` instead of `https://auth.openai.com/oauth/authorize`.
2. Wrong redirect URI: the registered redirect for the public Codex client is exactly `http://localhost:1455/auth/callback`; the build sends `http://127.0.0.1:<port>/callback`.
3. No ChatGPT account identity: the `chatgpt-account-id` request header must be derived from the access-token JWT claim `https://api.openai.com/auth` -> `chatgpt_account_id`, which nothing extracts or stores.

Separately, `GatewayTierMap` (per-`ReasoningTier` provider routing with fallback to the active provider) exists and is tested, but production always constructs it empty: there is no configuration surface, so declared workflow-step tiers can never reach a different provider.

## Proposed Solution

1. Correct the Codex OAuth flow: `/oauth/authorize` endpoint, fixed `http://localhost:1455/auth/callback` redirect (refuse to begin when the loopback listener could not bind exactly port 1455), the simplified-flow authorize parameters, account-id extraction from the access-token JWT at exchange and refresh time (fail closed when the claim is absent), and removal of the dead device-code functions whose endpoints never matched the real device-auth API.
2. Implement the ChatGPT backend Responses transport: new `ProviderKind::OpenaiCodex` mapping to a `ProviderClient::CodexResponses` client that POSTs the Responses-API request to `<base>/codex/responses` with the OAuth bearer token, `chatgpt-account-id`, `OpenAI-Beta: responses=experimental`, and `originator` headers, parses the SSE stream, and maps `response.failed`/`error` events to provider errors. The transport participates in the existing vault-token resolution and 401 refresh-once path. A dedicated Codex wire-fingerprint digest joins `provider_config_digest` so swap approvals bind the actual client surface, not the Anthropic one.
3. Enable the login: `openai-codex` joins `OAUTH_PROVIDER_IDS`, `login_supported` flips true, `/login openai-codex` works from terminal chat, the headless paste path accepts the full redirected URL (Codex has no hosted code page), and `openspine provider login <id>` re-binds instantly from a stored, non-disabled credential without a new browser round trip - that is the switch mechanism when both logins exist.
4. Owner-configurable tier routing: an optional `model_tiers` section in `openspine.yaml` (`low`/`standard`/`high` -> provider id) builds the production `GatewayTierMap`. Startup fails closed on a tier route naming an unknown provider. Unset tiers keep the current behavior (active provider). Tier resolution returns the provider id and client as one pair so the routed client always spends its own vault credential.

## Acceptance Criteria

- `openspine provider login openai-codex` produces an authorization URL with the corrected endpoint, redirect, and parameters; the token exchange stores refresh/access tokens plus the ChatGPT account id in the vault; a missing account-id claim refuses the login with a clear error.
- The gateway serves `model.generate` on a Codex credential end to end under test: correct URL/headers/body shape, SSE text extraction, `response.failed` mapped to a provider error, 401 triggering one refresh-and-retry.
- `openai-codex` is offered by the login chooser and `/login`; Google Antigravity remains refused.
- With stored credentials for both providers, `openspine provider login anthropic` (or `openai-codex`) re-binds the default provider without re-authorization.
- `model_tiers` routes a declared tier to its configured provider under test, and a routed OAuth provider spends its OWN vault credential end to end; a tier route to an unknown provider id fails startup; absent config preserves current routing. Production `model.generate` declares `standard` today, so `standard` routes are live immediately; `low`/`high` reach real calls when kernel workflow-step driving lands (deferred with D-090).
- All local gates pass: fmt, clippy `-D warnings`, workspace tests, file-size cap, claims check, `openspec validate --all --strict`.

## Out of Scope

- Google Antigravity login (still refused: no transport).
- Device-authorization-code login for Codex (the real device-auth API is bespoke; the paste-redirect-URL fallback covers headless).
- Streaming responses to the owner (the gateway remains request/response; SSE is consumed internally).
- Governed runtime model-swap ceremony changes (AD-152 machinery is untouched; config-file tier routes are owner-authored configuration, the same trust root as the provider list itself).
- Automatic model-id discovery; the operator names the model id explicitly per existing config convention.
