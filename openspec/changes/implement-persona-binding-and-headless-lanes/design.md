# Design: implement-persona-binding-and-headless-lanes

## Canon grounding

- **AD-136 (verbatim):** WHICH persona fronts a conversation is decided
  deterministically at route time from (connector/number × sender identity ×
  relationship), never by the agent. Personas are overlay artifacts
  (AD-080), but their BINDING is kernel machinery — and it is the same
  mechanism as hooks (AD-134): a phone number and a webhook are both
  event sources with route tables. Identity confusion is structurally
  impossible because binding happens before any agent code runs. Schema
  support already exists (RouteWhen matches source/connector/account_role/
  actor.relationship).
- **AD-134 (verbatim):** A hook is just another event source: verified,
  identified, routed, granted, gated — with no owner conversation
  anywhere in the loop when composed authority requires no approval. Working
  machinery runs silently; it surfaces only via the digest. How much
  surfaces is a learned overlay preference.
- **D-094..D-097 (landed):** persona is a seventh overlay artifact kind
  with no authority; admission is gated; personas never enter the
  authority composition path.

## Route schema extension (additive)

`RouteWhen` gains one optional field `channel_account: Option<String>`,
a deterministic equality match against `EventEnvelope.channel_account`
(the connector/number). `Route` gains one optional field
`persona: Option<ArtifactId>` carrying the persona artifact id the
*route* selects when it wins. Both are additive: absent fields impose
no constraint and existing routes serialize unchanged.

## Deterministic persona binding (kernel machinery)

A new pure `openspine_authority::persona_binding::resolve_persona`
function takes the event, the resolved `IdentityResolution`, the
`Option<RelationshipKind>`, the winning `Route`, and the loaded
`personas` registry, and returns `Option<ArtifactId>`:

1. If the winning `Route.persona` is `None`, binding yields `None`
   (no persona selected — the agent's default manifest persona applies).
2. If `Route.persona` names an id not present in the `personas`
   registry (or not `Active`), binding fails closed to `None` (a
   dangling or quarantined persona cannot front a conversation).
3. Otherwise the route-named persona id is returned.

The function is called only *after* deterministic route match success,
so a counterparty reaching an owner-bound number cannot select the
owner persona: the binding is derived from the matched route, not from
agent input. This mirrors the existing `resolve_route` purity (no I/O,
no lookup beyond the handed-in registry).

The pipeline driver threads the selected persona id into the composed
grant as `grant.persona_id` (a new additive field), so the kernel
record of "which persona fronted this conversation" is structural and
auditable, not a prompt detail.

## Headless hook lane (AD-134)

A new kernel module `pipeline::headless` drives a verified webhook
through the full governed pipeline as an *ordinary event source* — but
with `spawn_shell: false`, so the headless lane NEVER launches the
conversational shell: no owner conversation can be opened by a
webhook (a structural guarantee, not a policy toggle). Its effect is
driven through the non-conversational action executor.

- `HeadlessHookRequest { payload, signature, idempotency_key,
  signed_at, channel_account, action }` — the trusted inputs;
  `channel_account` is the authenticatable route selector, the rest
  are covered by the verifier MAC.
- `run_headless_hook(state, request, now)`:
  1. `WebhookVerifier::verify_bound` authenticates the
     `(payload, idempotency_key, channel_account, signed_at)` MAC
     and the replay window; failure → `webhook.rejected` audit, no
     grant minted. The replay cache is namespaced per
     `(channel_account, idempotency_key)` with a key-length cap and
     capacity eviction.
  2. Mints the `EventEnvelope` (`Source::Webhook`,
     `EventType::WebhookReceived`, `Lane::BusinessWorkflow`) and
     admits against the global daily spend cap.
  3. Calls `run_pipeline_with_envelope(... spawn_shell: false ...)`
     (Identify → Route → Compose → Grant). The headless `LaneSpec`
     `build_envelope`/`preflight` are no-ops because the envelope
     is already prebuilt and preverified; the `Run` stage is
     skipped (no shell is ever spawned).
  4. Dispatches the route's composed action via
     `mediate_and_dispatch_action_headless` — a headless variant that
     preserves `ApprovalRequired` (a standing rule can NEVER
     downgrade a mandatory headless approval to `Allow`).
  5. `Allow` → silent completion: `record_headless_hook_completion`
     + one owner-digest item, no Telegram conversation.
     `ApprovalRequired` → surfaced to the owner for approval; a
     no-approval action never opens a conversation.
- HTTP ingress: `POST /v1/webhooks/{channel_account}` in
  `api/webhook.rs` (wired via `webhook_routes()`) is the lane's
  real entry point. It authenticates by HMAC signature, not a
  bearer task token, and refuses when no HMAC key is configured
  (fail-closed).

## What this change deliberately does NOT do

- It does not make persona an authority-bearing kind.
- It does not let an overlay author the binding table; the binding is
  computed kernel-side from route + identity + relationship.
- It does not choose a fixed surfacing volume (learned overlay preference).
