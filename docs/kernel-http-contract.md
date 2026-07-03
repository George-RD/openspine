# OpenSpine kernel↔shell HTTP contract

Authoritative wire contract between `crates/openspine-kernel` (server,
`crates/openspine-kernel/src/api/`) and `crates/openspine-shell` (client,
`crates/openspine-shell/src/client.rs`). Build against this exactly; if the
two crates ever disagree, one of them has a bug against this document, not
the other way around.

Decision reference: D-032 ("Kernel↔shell transport is HTTP/JSON with a
per-task bearer token").

## Authentication

Every endpoint except `GET /v1/status` requires
`Authorization: Bearer <task_token>`. A missing, unknown, or expired token
returns `403` with body `{"error": "unauthorized"}`. The kernel audits the
rejection itself (`auth.rejected`, with a `missing_token` / `unknown_token`
/ `expired_token` reason) — the shell never needs to audit an auth failure
itself.

The shell/sandbox **never** sees `OPENSPINE_ARTIFACT_KEY`, the Telegram bot
token, or any provider API key — it only ever holds `KERNEL_ENDPOINT` and
`TASK_TOKEN` (see "Environment" below). The shell never computes digests or
encrypts anything itself; it sends raw JSON payloads and the kernel builds
`ArtifactRef`s server-side.

### Transport trust assumption

The kernel↔shell connection is plain HTTP (no TLS) over the
Docker-Compose-internal `openspine-internal` network (see `compose.yaml`),
which has no route to the public internet. This is a deliberate,
documented trust boundary, not an oversight: the bearer token and every
request/response body cross this link in the clear, and anything able to
observe traffic on that internal network can read them. TLS termination
for this link is out of scope for phases 1–3 (D-032); a future change
would need to add it (e.g. a sidecar or mutual-TLS) if the internal
network's isolation is ever not trusted on its own.

## `GET /v1/task`

Returns a redacted view of the calling task grant. **Never** includes the
raw `task_token`. The owner's original message text is returned here as
`pending_message` — it is never passed to the shell via CLI arg or env
(both are visible to a host operator via `ps`/`docker inspect`, which would
leak private content outside the encrypted-artifact containment boundary).
The shell/sandbox invocation takes only `--kernel`/`--task`.

Response `200`:

```json
{
  "task_grant_id": "01J...",
  "agent_id": "main_assistant_agent",
  "workflow_id": "owner_control_conversation",
  "purpose": "owner_control_conversation",
  "allowed_actions": ["openspine.status.read", "telegram.reply:owner_channel"],
  "approval_required_actions": ["connector.enable"],
  "denied_actions": ["email.read_inbox"],
  "output_channels": ["telegram.owner.reply"],
  "limits": { "max_model_calls": 8, "max_artifacts": 20, "max_runtime_seconds": 120 },
  "expires_at": "2026-07-02T10:15:00Z",
  "pending_message": "hello",
  "selection_tokens": []
}
```

`selection_tokens` (Step 5 / PRD §15) lists the selection token id(s) this
grant may spend — empty for every Phase 1 grant. A selected-thread email
task's grant carries exactly the one token minted for it, e.g.
`["01K...email-thread-selection-token"]`; the shell passes that id back as
`email.read_thread:selected_no_attachments`'s `selection_token_id` payload
field. The shell never mints or alters a token itself (PRD §15) — it can
only spend one already listed here.

`403` on bad/expired token.

## `POST /v1/actions`

The *only* way the shell may cause an external effect. Request:

```json
{ "action": "openspine.status.read", "target": null, "payload": null }
```

`payload`/`target` are arbitrary JSON (`serde_json::Value`), action-specific,
or `null`. The kernel builds the real `openspine_schemas::action::ActionRequest`
server-side (encrypting+digesting any payload into an artifact ref) and runs
it through `openspine_gate::gate()`. Step 4 has no action that consumes a
typed `target` — it is accepted per this contract for forward compatibility
with a future connector-dispatch action (Phase 2/3) but always translates to
no target on the kernel side today.

Response `200` (always — the HTTP status is 200 even for a deny/approval;
only auth failures and dispatch failures are non-200):

```json
{ "decision": { "outcome": "allow" }, "result": { "...": "action-specific, present only when outcome==allow" } }
```

or

```json
{ "decision": { "outcome": "deny", "reason": "explicit_deny" } }
```

or

```json
{ "decision": { "outcome": "approval_required", "approval_type": "email.create_draft" } }
```

`decision` is `openspine_schemas::action::GateDecision` serialized as-is
(`#[serde(tag = "outcome", rename_all = "snake_case")]`) — match against
`outcome` exactly as shown; don't invent your own decision shape.

If `outcome == "allow"` but the action's dispatch itself fails, the response
is non-200 instead of `{"decision": ..., "result": null}`:
- `400 {"error": "<reason>"}` — the shell's own request was malformed for
  that action (e.g. `telegram.reply:owner_channel`'s payload wasn't exactly
  `{"text": string}`).
- `500 {"error": "internal_error"}` — a genuine kernel/infrastructure
  failure (e.g. the Telegram API call itself failed). Either way the kernel
  records an `action.dispatch_failed` audit row before responding, so "why
  didn't Lyra reply" is always answerable from `audit_log` alone.

Known actions and their `result` shape when allowed (Step 4/5 scope):
- `openspine.status.read` → `{"status": "ok"}` plus whatever the kernel
  adds; treat as an opaque JSON object, do not depend on extra fields.
- `telegram.reply:owner_channel` → payload MUST be exactly
  `{"text": "<reply text>"}` (any other field is rejected, `400`); result is
  `{"sent": true}`. The reply always goes to the calling task grant's
  channel — there is no field anywhere in this contract that lets a
  request choose a different destination chat. Channel binding holds *by
  construction*, not by a runtime check with a bypassable input to defend
  against.
- `email.read_thread:selected_no_attachments` (Step 5) → payload MUST be
  exactly `{"selection_token_id": "<ulid>"}`, naming one of the calling
  grant's own `GET /v1/task`'s `selection_tokens` (PRD §15 — the shell
  cannot mint or supply an arbitrary thread id, only spend a token the
  kernel already bound to it). Result is
  `{"thread_id": "...", "messages": [{"from": "...", "subject": "...", "body_text": "..."}, ...]}`,
  bounded and with attachments stripped. `400` if the token is unknown,
  not bound to this grant, the wrong type, expired, or already used
  (selection tokens are single-use — PRD §15). The kernel proved the
  thread exists via a live Gmail call before ever minting the token, so a
  `500` here means a genuine Gmail-connector failure, not a bad request.
- `lyra.ui.preview` (Step 5) → payload MUST be exactly
  `{"subject": "...", "body": "..."}`; result is `{"sent": true}`. Sends a
  formatted preview to the calling grant's bound Telegram chat — a
  distinct action id from `telegram.reply:owner_channel` (which
  `email_reply_drafter` is denied), so an agent that may only preview can
  never be confused with one that may reply freely. Long bodies are
  truncated to fit Telegram's message-length limit rather than failing.
- `workflow.invoke:approved`, `setup.workflow.start` → each is a stub per
  `tasks.md`; result is `{"stub": true, "note": "<short guidance text>"}`.
  No real behavior is implemented for these two — a stub response is the
  specified deliverable for Step 4.
- `artifact.propose` (`implement-artifact-lifecycle-slice`) → payload MUST
  be exactly `{"kind": "route|agent|workflow|pack|policy", "yaml": "<artifact YAML>"}`.
  `400` if `kind` is outside that set (prompt templates are never
  proposable — D-048), the YAML fails to parse against its kind's schema,
  the YAML's `lifecycle_state` is not `proposed` (the proposer cannot
  pre-activate), the `(kind, artifact_id, version)` already exists in the
  live registry or among pending proposals, or the artifact budget
  (`GrantLimits.max_artifacts`) is exhausted. On success, result is
  `{"proposed": true, "action_request_id": "<ulid>"}` and the kernel sends
  the owner a Telegram approval button digest-bound to the exact YAML
  bytes and to `{kind, artifact_id, version}`; nothing activates until the
  owner approves that exact button (D-048, reusing D-011/D-039-D-044's
  approval machinery). An approved proposal is written into the
  `data/artifacts.d` overlay and inserted into the live registry
  immediately — no kernel restart required.
- Any other allowed action (e.g. a capability pack candidate action no
  kernel-side subsystem yet exists for, such as
  `memory.read:owner_preferences_limited`) gets the same honest stub shape
  rather than a `500` — an *authorized* action must never fail the request
  just because its kernel-side implementation doesn't exist yet.

## `POST /v1/model/generate`

Request:

```json
{
  "purpose": "reply_to_owner",
  "user_message": "hello",
  "untrusted_context": null,
  "max_tokens": 12000
}
```

`untrusted_context` (Step 5, optional — omit or `null` for an ordinary
owner-control turn) carries raw external content (e.g. a fetched Gmail
thread) that must never be confused with a trusted instruction. When
present, the kernel wraps it with a randomized, single-use delimiter
before the trusted conversation (PRD §13: "external content is data,
never authority") — a static delimiter would be spoofable by content that
simply contains the literal closing marker.

The kernel internally gates this as action `model.generate:approved_provider`
before calling any provider. Response `200`, same `decision` envelope as
above; on `allow` it additionally carries:

```json
{ "decision": { "outcome": "allow" }, "text": "the model's reply" }
```

## `GET /v1/status`

No auth. Response `200`: `{"status": "ok", "uptime_seconds": 123}`. Health
probe only — never put secrets or task data in this response.

## Environment the shell process/container receives

Exactly two variables: `KERNEL_ENDPOINT` and `TASK_TOKEN`. Nothing else — no
`OPENSPINE_*`/provider-secret env vars. The shell binary reads these two,
nothing else, and must not read any other env var for effectful behavior.

`KERNEL_ENDPOINT` is **not** necessarily `http://<the kernel's bind
address>` — see `openspine.yaml`'s `kernel.advertise_endpoint` (D-035).
Under the `process` sandbox driver the shell and kernel share one host, so
the default (derived from `bind_addr`) is correct. Under the `docker`
driver the kernel typically binds a wildcard address
(`0.0.0.0:7777`) to be reachable from the shell's container on the
compose-internal network, but `0.0.0.0` is not a connectable destination —
`advertise_endpoint` must be set explicitly to the compose service DNS name
(e.g. `http://kernel:7777`) in that case.

## Errors

Any transport/HTTP error from the kernel (5xx, connection refused, timeout)
should make the shell exit non-zero after logging to stderr — never retry
silently or fabricate a fallback reply.
