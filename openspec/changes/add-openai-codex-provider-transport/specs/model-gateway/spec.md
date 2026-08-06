# model-gateway delta

## ADDED Requirements

### Requirement: Codex OAuth grants MUST be spent through the ChatGPT backend Responses transport

A provider of kind `openai_codex` MUST be served by a dedicated transport that POSTs a Responses-API request to `<base>/codex/responses` (default base `https://chatgpt.com/backend-api`) with `Authorization: Bearer <access token>`, `chatgpt-account-id`, `OpenAI-Beta: responses=experimental`, and `originator` headers, `store:false` and `stream:true` in the body, and consumes the SSE stream kernel-side (LF and CRLF framing alike). Text MUST be accumulated from `response.output_text.delta` events; every terminal event (`response.completed`/`done`/`incomplete`) supplies the output items as a fallback when no deltas arrived. `response.failed` and `error` events MUST map to provider errors carrying the upstream message; a terminal event whose nested response status reports `failed` or `cancelled` MUST be rejected whatever frame it arrived in; an `incomplete` terminal with no usable text MUST fail with its reported reason. A Codex call with no stored account id MUST fail with an error naming the re-login remedy instead of sending a request the backend will reject. The transport MUST participate in the existing vault token resolution and 401 refresh-once path, and the exact access token and account id strings MUST be scrubbed from any error it returns. A Codex-specific wire-fingerprint digest covering the identifying client surface the transport consumes (originator, beta, user agent, endpoint path, accept header) MUST participate in `provider_config_digest` for `openai_codex` OAuth providers; the request body shape is pinned by the transport's wire tests.

#### Scenario: Codex generate round trip

Given an `openai_codex` provider with a vault credential and account id
When the gateway serves a `model.generate`
Then the request MUST target `/codex/responses` with the bearer token, account id, beta, and originator headers
And the body MUST carry `store:false`, `stream:true`, the template system text as `instructions`, and the conversation as typed input items
And the reply text MUST be assembled from the SSE stream.

Test: `codex_generate_sends_the_registered_wire_shape_and_reads_sse`, `codex_generate_falls_back_to_completed_output_when_no_deltas_arrive`

#### Scenario: Upstream failure surfaces verbatim

Given the Codex backend answers the SSE stream with a `response.failed` event
When the gateway parses the stream
Then the call MUST fail with a provider error carrying the upstream failure message
And a terminal event whose nested status reports `failed` MUST be rejected even when deltas arrived
And an `incomplete` terminal with no usable text MUST fail naming its reported reason.

Test: `codex_generate_maps_response_failed_to_a_provider_error`, `a_terminal_event_with_failed_status_is_rejected_despite_deltas`, `an_incomplete_terminal_without_text_fails_with_its_reason`, `an_incomplete_terminal_with_text_returns_the_fragment`

#### Scenario: Expired access token recovers once

Given a Codex call rejected with HTTP 401
When the provider is OAuth-configured and a refresh succeeds
Then the gateway MUST retry the call exactly once with the refreshed token.

Test: `codex_generate_retries_once_after_a_401_via_inline_refresh`

#### Scenario: Missing account identity fails closed

Given an `openai_codex` provider whose vault holds no account id
When a generate is attempted
Then the call MUST fail with an error naming `openspine provider login openai-codex` as the remedy
And no request MUST reach the backend.

Test: `codex_generate_refuses_when_no_account_id_is_stored`

#### Scenario: Error bodies never echo credential material

Given a backend error body that echoes the bearer token or account id
When the gateway surfaces the failure
Then the exact secret strings MUST be replaced with `<redacted>` in the returned error.

Test: `codex_error_bodies_never_echo_the_bearer_or_account_id`

### Requirement: Reasoning-tier provider routing MUST be owner-configurable and fail closed

The kernel MUST accept an optional `model_tiers` configuration mapping reasoning tiers (`low`, `standard`, `high`) to provider ids and MUST build the gateway tier map from it. A tier route naming a provider id absent from the configured provider list MUST abort startup with the offending name. Tiers without a route MUST fall back to the active provider for the role, preserving existing behavior when the section is absent. Tier resolution MUST return the provider id and client as one pair, and credential resolution MUST key on the routed id — a routed client spending another provider's vault credential is unrepresentable at the call site. Production `model.generate` declares the `standard` tier today; per-step declared tiers reach the map through `WorkflowStateMachine::provider_for_step` when kernel workflow-step driving lands (deferred with D-090).

#### Scenario: A declared tier reaches its routed provider

Given `model_tiers.high: anthropic` and an active provider `openai-codex`
When a generation carries `ReasoningTier::High`
Then the gateway MUST resolve the `anthropic` client from the pool paired with the `anthropic` id
And a generation carrying `ReasoningTier::Standard` MUST resolve the active provider paired with its id.

Test: `configured_tier_routes_reach_their_provider_and_others_fall_back`

#### Scenario: A routed provider spends its own credential

Given OAuth credentials stored for both the active provider and a `model_tiers.standard` routed provider
When a production `model.generate` is served
Then the routed endpoint MUST receive the routed provider's own vault token and account id
And the active provider's endpoint MUST receive nothing.

Test: `a_tier_routed_oauth_provider_spends_its_own_credential`

#### Scenario: A tier route to an unknown provider refuses startup

Given `model_tiers.low: no-such-provider`
When the kernel validates configuration at startup
Then startup MUST fail naming `no-such-provider`
And no server MUST be started.

Test: `a_tier_route_to_an_unknown_provider_fails_config_validation`
