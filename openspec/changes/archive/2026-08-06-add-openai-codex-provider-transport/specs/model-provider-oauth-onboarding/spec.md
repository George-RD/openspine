# model-provider-oauth-onboarding delta

## MODIFIED Requirements

### Requirement: Interactive setup wizard MUST execute OAuth PKCE authorization

The interactive setup CLI (`openspine setup`, `openspine provider login`) MUST initiate an OAuth 2.0 PKCE authorization flow for the selected provider. It MUST generate a cryptographically secure random code verifier, derive the `S256` code challenge, open the provider authorization URL in a local browser when available, and bind a loopback HTTP callback listener to receive the authorization code. When a provider's registered redirect URI is fixed (host, path, and port), the CLI MUST send exactly that redirect URI and MUST refuse to begin authorization when the loopback listener could not bind the registered port.

#### Scenario: User initiates OAuth login for Google Antigravity

Given a user selects Google Antigravity in `openspine setup`
When the OAuth PKCE flow initiates
Then the CLI MUST generate a valid S256 code challenge and state parameter
And it MUST open the Google authorization URL pointing to `127.0.0.1:51121/callback`
And it MUST receive the authorization code on the local callback listener.

Test: `oauth_pkce_generator_computes_valid_s256_challenge`, `oauth_loopback_callback_server_receives_code_and_validates_state`

#### Scenario: User initiates OAuth login for OpenAI Codex

Given a user selects OpenAI Codex in `openspine provider login`
When the OAuth PKCE flow initiates
Then the authorization URL MUST target `https://auth.openai.com/oauth/authorize`
And it MUST carry redirect URI `http://localhost:1455/auth/callback` exactly
And it MUST carry the `codex_cli_simplified_flow`, `id_token_add_organizations`, and `originator` parameters.

Test: `codex_authorization_url_matches_registered_client_contract`

#### Scenario: Registered redirect port is unavailable

Given port 1455 is already bound by another process
When `openspine provider login openai-codex` attempts to begin authorization
Then the flow MUST refuse before producing an authorization URL
And the refusal MUST name the fixed redirect `http://localhost:1455/auth/callback`.

Test: `codex_login_refuses_a_port_the_registered_redirect_does_not_cover`

### Requirement: OAuth login MUST support headless and remote SSH fallbacks

When local browser opening fails or an SSH/headless environment is detected, the CLI MUST provide a fallback mechanism. It MUST display the authorization URL with clear instructions and support manual paste of the authorization response: a bare code, a `code#state` pair, or the full redirected callback URL. When the pasted input carries a `state`, it MUST match the flow's state. Device Authorization Code polling MAY be offered only for providers whose device endpoints are actually implemented.

#### Scenario: Headless SSH environment uses manual code input

Given an SSH environment where local browser launch is unavailable
When `openspine provider login` runs
Then the CLI MUST print the authorization URL
And it MUST prompt for manual authorization code paste
And upon input, it MUST complete token exchange successfully.

Test: `oauth_login_fallback_accepts_manual_authorization_code_input`

#### Scenario: Headless Codex login pastes the redirected URL

Given a Codex authorization completed in a browser on another machine
When the owner pastes the full `http://localhost:1455/auth/callback?code=...&state=...` URL at the prompt
Then the CLI MUST extract the code and state from the URL
And a state that does not match the flow's state MUST be refused.

Test: `pasted_redirect_url_yields_code_and_checks_state`

## ADDED Requirements

### Requirement: OpenAI Codex login MUST derive the ChatGPT account identity from the access token

The Codex token exchange and token refresh MUST decode the access-token JWT payload and extract the `https://api.openai.com/auth` -> `chatgpt_account_id` claim. A token carrying no account id MUST fail the login closed with an error naming the missing claim, because the resulting credential could never be spent; the same condition on a background refresh MUST disable the credential as a definitive failure rather than retrying forever. The account id MUST be stored as encrypted identity metadata alongside the tokens and MUST survive access-token refreshes, whether or not the refresh rotated the refresh token. A stored credential lacking the account id (an older build's login) MUST be backfilled from the stored access token's claim at re-bind time; when the claim is absent there too, the credential is incomplete and a fresh authorization runs.

#### Scenario: Exchange stores the account identity

Given a Codex token exchange returning an access token whose JWT carries `chatgpt_account_id`
When the login completes
Then the vault MUST hold the account id under the provider's identity metadata
And subsequent refreshes MUST NOT erase it.

Test: `codex_exchange_extracts_chatgpt_account_id_from_access_token`, `refresh_preserves_stored_account_identity`

#### Scenario: Access token without account claim is refused

Given a token exchange returning an access token with no `chatgpt_account_id` claim
When the login attempts to store the credential
Then the login MUST fail with an error naming the missing account identity
And no credential MUST be bound to model roles.

Test: `codex_exchange_refuses_a_token_with_no_account_id`

#### Scenario: An identity-less stored credential is backfilled at re-bind

Given a vault credential stored by an older build with no account id
When `openspine provider login openai-codex` takes the stored-credential path
Then the account id MUST be backfilled from the stored access token's claim
And the re-bind MUST proceed with the backfilled identity.

Test: `rebind_backfills_a_missing_codex_account_id_from_the_stored_token`

### Requirement: A stored credential MUST allow re-binding without a new authorization

When `openspine provider login <id>` names a provider whose vault credential is present, not disabled, and holds a refresh token, the CLI MUST skip the authorization round trip, re-verify the provider through the model gateway, and re-bind it as the routed provider on success. This is the switching path when credentials for multiple providers are stored; `--force` re-runs the authorization. The shortcut MUST NOT outflank the spendability refusal: a provider whose `login_supported` is false stays refused with a stored credential exactly as without one. Re-binding a provider id whose configured entry carries a different kind MUST cut the entry over to the canonical transport kind and reset its endpoint to the canonical default, so a credential is never sent to an endpoint configured for a different wire contract.

#### Scenario: Switching between two held subscription logins

Given non-disabled vault credentials for both `anthropic` and `openai-codex`
When the owner runs `openspine provider login anthropic`
Then no authorization URL MUST be produced
And the provider MUST be verified through the gateway and re-bound in `openspine.yaml`.

Test: `login_with_a_stored_credential_rebinds_without_a_new_authorization`

#### Scenario: A stored credential for an unsupported provider stays refused

Given a stored, non-disabled credential for `google-antigravity`
When the owner runs `openspine provider login google-antigravity`
Then the login MUST refuse before any verification or binding.

Test: `a_stored_credential_for_an_unsupported_provider_is_still_refused`

#### Scenario: A legacy transport entry is cut over on binding

Given a configured `openai-codex` entry written by an older build with kind `openai_compat` and a custom endpoint
When the provider is verified and re-bound
Then the entry MUST carry the canonical `openai_codex` kind and the canonical default endpoint.

Test: `a_legacy_compat_codex_entry_is_cut_over_to_the_canonical_transport`
