# Lyra PRD Companion — Decisions Log

**Purpose:** This companion log captures the reasoning behind major Lyra PRD decisions so future agents, implementers, or reviewers can understand why the spec is shaped the way it is before changing it.

**Status:** Working companion to Lyra PRD v8/v9 direction.

**How to use this document:**
Before changing a PRD section, check the relevant decision entry. If the proposed change reverses a decision, add a new decision entry explaining why the old rationale no longer holds.

---

## Decision Index

| ID    | Decision                                                                  | Current stance |
| ----- | ------------------------------------------------------------------------- | -------------- |
| D-001 | Lyra is a runtime/substrate, not a single agent                           | Accepted       |
| D-002 | First usable UX should include an owner control channel                   | Accepted       |
| D-003 | Gmail is a guarded workflow, not the whole product                        | Accepted       |
| D-004 | Every effectful action goes through `gate()`                              | Accepted       |
| D-005 | Private-data shell must be contained                                      | Accepted       |
| D-006 | Identity is not authority                                                 | Accepted       |
| D-007 | Task grant is the final runtime authority                                 | Accepted       |
| D-008 | Deterministic routing decides authority; agentic routing decides strategy | Accepted       |
| D-009 | External content is data, not instruction                                 | Accepted       |
| D-010 | Model calls with private context go through model gateway                 | Accepted       |
| D-011 | Approval must be digest-bound                                             | Accepted       |
| D-012 | Audit stores private payloads by encrypted/hash reference                 | Accepted       |
| D-013 | Dynamic behavior should be easy; dynamic authority should be hard         | Accepted       |
| D-014 | Bootstrap/setup secrets bypass shell/model context                        | Accepted       |
| D-015 | Phase 1 should avoid final email send                                     | Accepted       |
| D-016 | Capability packs are candidate profiles, not live authority               | Accepted       |
| D-017 | Personas grant no authority                                               | Accepted       |
| D-018 | Routes are declarative artifacts, not kernel code                         | Accepted       |
| D-019 | Implement minimal slice first, not full agent OS                          | Accepted       |
| D-020 | Railway/Docker/VPS are deployment targets, not core architecture          | Accepted       |
| D-021 | Email domain is broader than Gmail                                        | Accepted       |
| D-022 | Agent-owned inbox is distinct from owner mailbox access                   | Accepted       |
| D-023 | OpenSpine is the substrate; Lyra is a product built on it                 | Accepted       |
| D-024 | OpenSpec is the development/change-management layer, not the runtime      | Accepted       |
| D-025 | Rust/Tokio substrate: storage, audit chain, and secrets handling          | Accepted       |
| D-026 | Shell containment via a `SandboxDriver` trait (Process dev-only / Docker) | Accepted       |
| D-027 | Multi-provider model gateway with per-provider auth mode                  | Accepted       |
| D-028 | Canonical artifact format is YAML+serde; digests are `sha256:` over canonical JSON | Accepted |
| D-029 | Gmail OAuth scopes: `readonly` + `compose`, send hard-denied at the gate  | Accepted       |
| D-030 | Telegram carries the entire owner-control UX for phases 1–3              | Accepted       |
| D-031 | Docker Compose is the first reference deployment target                  | Accepted       |
| D-032 | Kernel↔shell transport is HTTP/JSON with a per-task bearer token          | Accepted       |
| D-033 | Action identifiers are exact-match dotted strings; unverified senders are audited and ignored | Accepted |
| D-034 | `email.create_draft` is the one canonical action id (PRD §10.2's qualified spelling dropped) | Accepted |

---

# D-001 — Lyra is a runtime/substrate, not a single agent

## Decision

Lyra should be described as an event-driven, identity-aware, capability-gated runtime for personal agents, not as “the Lyra agent.”

## Rationale

Calling Lyra an agent implies a single smart assistant with broad authority. The architecture is instead a substrate that receives events, resolves identity, routes work, grants bounded capabilities, runs agents/workflows, mediates effects, and audits outcomes.

## Trade-offs

| Option                    | Benefit                                  | Risk                                   |
| ------------------------- | ---------------------------------------- | -------------------------------------- |
| Lyra as main agent        | Easier for users to understand initially | Encourages god-agent authority model   |
| Lyra as runtime/substrate | More accurate and scalable               | More abstract; needs clearer UX naming |

## Consequences

- The default assistant should be called something like `main_assistant_agent`, not “Lyra itself.”
- Agents run inside Lyra; they do not own Lyra.
- Kernel, routing, policy, connectors, audit, and vault remain outside agent authority.

## Would change if

A future product decision intentionally narrows Lyra into a single assistant app rather than a general runtime.

---

# D-002 — First usable UX should include an owner control channel

## Decision

The first usable version should include a verified owner control channel, likely Telegram first, rather than being only a Gmail selected-thread workflow.

## Rationale

A Gmail-only first version proves a guarded workflow but does not feel like an agent system. The common user interaction pattern is messaging the main assistant through Telegram, WhatsApp, CLI, web, or eventually a native app.

Telegram is a practical first owner channel because bot setup is simpler than WhatsApp and it gives immediate conversational control.

## Trade-offs

| Option                                  | Benefit                              | Risk                                       |
| --------------------------------------- | ------------------------------------ | ------------------------------------------ |
| Gmail-only first                        | Narrowest containment proof          | Feels like a Gmail tool, not agent runtime |
| Telegram-first control + Gmail workflow | Feels like agent system from day one | Adds one extra connector/event type        |

## Consequences

- Add event type `telegram.owner.message`.
- Add `main_assistant_agent` as owner-facing orchestrator.
- Gmail selected-thread drafting remains the first guarded external-content workflow.

## Would change if

Telegram setup proves too distracting or materially delays the containment proof. In that case, CLI could temporarily act as the owner control channel.

---

# D-003 — Gmail is a guarded workflow, not the whole product

## Decision

Gmail selected-thread drafting is the first guarded workflow, not the architecture itself.

## Rationale

Email is high-risk because it contains external, potentially hostile content and prompt-injection attempts. Treating Gmail as the main assistant interface would blur trusted owner instruction with untrusted external data.

## Trade-offs

| Option                    | Benefit                      | Risk                                         |
| ------------------------- | ---------------------------- | -------------------------------------------- |
| Gmail as core app         | Clear productivity use case  | Narrows product and increases injection risk |
| Gmail as guarded workflow | Safer and generalizes better | Requires separate owner control UX           |

## Consequences

- Email content routes to `email_reply_drafter` or similar specialist workflow.
- The main assistant may invoke or coordinate the Gmail workflow but should not ingest arbitrary email directly.
- Final email send is excluded from early phases.

## Would change if

A later phase has mature prompt-injection handling, stronger sandboxing, and approval UX sufficient for broader email automation.

---

# D-004 — Every effectful action goes through `gate()`

## Decision

All effectful actions must be mediated by `gate()`.

## Rationale

“State-changing action” was too narrow. Reads, model calls, memory access, network calls, filesystem access, and credential use can expose data or influence future state even if they do not mutate an external system.

## Effectful actions include

- external reads
- external writes
- private model calls
- memory reads/writes
- tool calls
- network calls
- filesystem access
- credential use
- policy/capability requests
- generation or artifact activation
- evaluator/holdout access

## Consequences

- Agents submit typed action requests.
- Kernel-owned connectors execute approved requests.
- Reads are treated as first-class risks.

## Would change if

A future formal model identifies a class of purely local, non-persistent, non-private operations that can safely bypass gate. Default remains gate-mediated.

---

# D-005 — Private-data shell must be contained

## Decision

Any process receiving private user data must have no unmediated exfiltration paths.

## Rationale

Removing credentials from the shell is not enough. Once private email/message content is disclosed to the shell, credentials are no longer the main risk; exfiltration is.

## Required containment

- no raw connector credentials
- no arbitrary network egress
- no direct external model access
- no unrestricted filesystem access
- no host secrets in environment
- no direct control/eval/audit DB access
- redacted and size-limited logs
- supervised process/container

## Consequences

- Shell calls kernel local API only.
- Model calls go through model gateway.
- Official connectors are kernel-owned.
- If containment cannot be enforced, use synthetic/redacted data.

## Would change if

The system runs only on fully synthetic data or in a trusted development mode explicitly marked unsafe for private content.

---

# D-006 — Identity is not authority

## Decision

Identity records store knowledge about people/entities. They do not directly grant authority.

## Rationale

Cross-channel identity is useful but dangerous. The same person may contact through email, WhatsApp, Telegram, Slack, or Discord, but each channel has different trust and spoofing characteristics.

## Consequences

- Identity records should not contain live `capability_pack_id` grants in phase 1.
- Authority is derived at runtime from event authenticity, identity confidence, channel trust, route, policy, agent manifest, workflow, capability pack, and user/session policy.
- Probabilistic matches may suggest candidates but cannot unlock trusted routes without verification or user confirmation.

## Would change if

A future identity system has strong cryptographic identity proof across channels. Even then, channel policy should remain separate.

---

# D-007 — Task grant is the final runtime authority

## Decision

The task grant is the only live authority object presented to a running agent/workflow.

## Rationale

Routes, agents, workflows, identity records, and capability packs are inputs to authority. If each independently grants authority, composition becomes ambiguous and unsafe.

## Consequences

- Running agents receive task grants, not broad permissions.
- Grants are short-lived, purpose-bound, route-bound, agent-bound, and target-bound.
- Grants bind exact artifact versions/digests where possible.

## Would change if

A simpler trusted single-user prototype is intentionally built without strong runtime authority. That would be a different product mode, not the main architecture.

---

# D-008 — Deterministic routing decides authority; agentic routing decides strategy

## Decision

Authority-affecting routing must be deterministic. Agentic routing may operate only inside an already-approved authority envelope.

## Rationale

LLMs should not decide their own permissions, identity trust, or whether external effects are allowed.

## Deterministic decisions

- source verification
- identity confidence threshold
- route conflict resolution
- capability pack selection
- approval requirement
- task grant construction

## Agentic decisions

- drafting strategy
- whether to ask clarification
- which bounded skill to call
- how to summarize
- proposing new artifacts for review

## Would change if

A future verified policy-reasoning engine can produce auditable deterministic outputs. LLM free-form judgment remains unsuitable for authority.

---

# D-009 — External content is data, not instruction

## Decision

External content must be treated as data, not instruction.

## Rationale

Email, web pages, attachments, customer messages, and unknown inbound messages may contain prompt injections or social engineering. They should not be allowed to modify system behavior or authority.

## Examples

| Source                            | Trust posture                       |
| --------------------------------- | ----------------------------------- |
| Verified owner Telegram message   | Instruction candidate               |
| Gmail thread from external sender | Data only                           |
| Web page                          | Hostile data                        |
| Attachment                        | Hostile data until parsed/sandboxed |
| Unknown WhatsApp/SMS sender       | Low-trust inbound content           |

## Consequences

- Email routes to guarded workflows.
- Main assistant should not ingest arbitrary external content as instruction.
- Tool-output injection defenses are core, not optional.

## Would change if

External content is produced by a verified trusted system under explicit contract. Even then, it should be scoped.

---

# D-010 — Model calls with private context go through model gateway

## Decision

The shell does not call external model providers directly when private data is involved. It requests inference through the model gateway.

## Rationale

Private model calls are effectful actions. If the shell can directly call OpenAI/Anthropic/etc. with private context, model export bypasses policy.

## Gateway responsibilities

- resolve input refs
- apply trusted prompt templates
- enforce redaction/data policy
- choose/validate provider/model
- enforce retention policy
- size-limit input/output
- store prompt/output as encrypted artifacts
- return only allowed output
- audit metadata and refs

## Would change if

Only a fully local model is used inside the contained runtime and no private data leaves the host. Even then, model calls should still be audited.

---

# D-011 — Approval must be digest-bound

## Decision

Approval applies to the exact immutable payload and target the user reviewed.

## Rationale

Without digest binding, a shell could show draft A, receive approval, then mutate to draft B before execution.

## Consequences

- Drafts are stored as immutable artifacts.
- Approval records payload digest and target digest.
- Kernel executes only the approved artifact.
- Any body, recipient, target, or thread change invalidates approval.

## Would change if

The approved action is purely internal, reversible, and low risk. External writes should remain digest-bound.

---

# D-012 — Audit stores private payloads by encrypted/hash reference

## Decision

Audit stores metadata directly but private payloads as encrypted artifact refs and hashes.

## Rationale

The audit system should not become the largest plaintext privacy risk.

## Consequences

- Raw email bodies, model prompts, model outputs, draft bodies, and corrections are not stored as raw audit text.
- Audit verification survives deletion of raw payloads through retained hashes.
- Artifact store becomes security-sensitive and needs encryption, retention, access controls, and backup rules.

## Would change if

User explicitly opts into full plaintext local audit for debugging. This should be unsafe/dev mode only.

---

# D-013 — Dynamic behavior easy; dynamic authority hard

## Decision

Lyra should make it easy to add routes, workflows, agents, skills, and personas, but hard to increase authority.

## Rationale

The value of an agent OS is adaptability. The risk is capability creep. Separating behavior from authority allows growth without losing control.

## Consequences

- Agents may propose artifacts.
- Authority-preserving or narrowing changes can have lighter approval.
- Widening requires explicit human approval.
- New connectors, broader reads, external writes, weaker approval rules, and lower identity thresholds are widening events.

## Would change if

A future managed enterprise version adds centralized admin policy. The principle remains, but approval authority changes.

---

# D-014 — Bootstrap/setup secrets bypass shell/model context

## Decision

Setup secrets must be captured by a vault/secret-intake flow, not by ordinary agent chat.

## Rationale

Users may paste Telegram bot tokens, API keys, OAuth credentials, or setup secrets. If these pass through the LLM or normal chat memory, they can leak into logs, traces, model providers, or memory.

## Consequences

- Add secret-intake mode.
- Next user message can be routed directly to vault capture.
- Agent sees only success/failure metadata, not the secret.
- No model call is made with the secret.
- Audit logs “secret received/validated/stored,” not the token.

## Would change if

All setup credentials are provided only through environment variables or OAuth redirects. Secret-intake remains useful for later connectors.

---

# D-015 — Phase 1 should avoid final email send

## Decision

Final email sending is excluded from early phases.

## Rationale

Sending email is third-party-visible and compensating-only. It cannot be truly rolled back. Drafting is enough to prove private data handling, model gateway, approval UX, audit, and containment.

## Consequences

- No `gmail.send` connector in phase 1.
- Gmail draft creation comes only after digest-bound approval is proven.
- If OAuth scopes technically allow send, kernel policy and tests must prove send is denied.

## Would change if

A later phase has mature approval UX, strong recipient validation, reliable audit, and explicit user opt-in for send actions.

---

# D-016 — Capability packs are candidate profiles, not live authority

## Decision

Capability packs contribute candidate permissions and constraints, but do not grant live authority by themselves.

## Rationale

Reusable profiles are needed for elegance, but if attaching a pack grants authority directly, authority becomes hard to reason about.

## Consequences

- Capability packs are inputs to authority composition.
- The task grant materializes final authority.
- Explicit denies and approval requirements are preserved through composition.

## Would change if

A very simple prototype treats packs as direct grants. That should be marked as a shortcut and not used for private data.

---

# D-017 — Personas grant no authority

## Decision

Personas affect style and behavior only. They grant no capabilities.

## Rationale

Names like “CEO assistant,” “senior operator,” “lawyer,” or “admin” can imply authority socially. They must not imply technical authority.

## Consequences

- Persona may influence tone, reasoning style, and interaction pattern.
- Persona cannot add tools, memory, connector access, or external write authority.

## Would change if

Never for core architecture. Authority must remain separate.

---

# D-018 — Routes are declarative artifacts, not kernel code

## Decision

Routes should be versioned declarative artifacts, not hard-coded kernel branches.

## Rationale

Future use cases require adding routes like “messages from X go to Agent B” without kernel changes. The kernel should validate route artifacts and authority composition, not know business-specific routing logic.

## Consequences

- Route artifacts have lifecycle states.
- Ambiguous route matches fall back to low-authority triage/review.
- LLMs may propose routes but cannot activate authority-widening routes.

## Would change if

A minimal prototype hard-codes the first Gmail route internally. That should still mirror the route artifact schema and be refactored quickly.

---

# D-019 — Implement minimal slice first, not full agent OS

## Decision

Define general schemas, but implement the smallest useful slice first.

## Rationale

The architecture is broad. Building the full identity graph, multi-channel router, marketplace, evolution loop, and foundation amendment lane before a useful workflow would create design churn.

## Minimal slice

- owner identity
- Telegram owner control event, if included in v1
- Gmail selected-thread event
- one route
- one agent manifest
- one workflow
- one capability pack
- one task grant
- contained shell
- Gmail read connector
- model gateway
- local preview
- audit verify

## Would change if

A team with more capacity is building in parallel. Even then, the first integration test should remain minimal.

---

# D-020 — Railway/Docker/VPS are deployment targets, not core architecture

## Decision

Deployment targets should not define the architecture.

## Rationale

Railway one-click deployment is attractive for adoption, while Docker/VPS is useful for self-hosting. The runtime should remain deployment-agnostic.

## Consequences

- Railway may be a product onboarding path.
- Docker Compose should likely be the reference self-hosted path.
- Local dev should remain possible.
- Deployment-specific secret handling must map into the same vault/bootstrap model.

## Would change if

The product intentionally becomes a managed hosted service first. Even then, core concepts should remain portable.

---

# D-021 — Email domain is broader than Gmail

## Decision

The PRD should describe the domain as **email** or **external communication**, with Gmail treated as the first concrete email connector.

## Rationale

Gmail is popular and useful for testing, but the architecture should support multiple email contexts: personal Gmail, Google Workspace, Outlook, IMAP, dedicated agent inboxes, shared mailboxes, and future email providers. Calling the workflow “Gmail drafting” is acceptable only when discussing the first implementation.

## Trade-offs

| Option                 | Benefit                              | Risk                                      |
| ---------------------- | ------------------------------------ | ----------------------------------------- |
| Gmail-specific wording | Concrete and easy to implement first | Overfits the architecture to one provider |
| Email-domain wording   | General and future-proof             | Slightly less concrete for first build    |

## Consequences

- Use “selected-thread email reply drafting” for the workflow.
- Use “Gmail” only as the first email connector implementation.
- Google Workspace is an account/connector context, not a new architecture.
- Future email connectors can reuse the same event/route/capability/task-grant model.

## Would change if

Lyra intentionally narrows to Google-only integrations. Current direction is provider-agnostic.

---

# D-022 — Agent-owned inbox is distinct from owner mailbox access

## Decision

Lyra should distinguish **owner mailbox access** from **agent-owned inboxes**.

## Rationale

An agent reading or drafting from the user’s personal/work mailbox has different risk from an agent operating its own email address. Agent-owned inbox providers such as AgentMail give agents real programmatic inboxes for sending, receiving, threading, search, webhooks, custom domains, and use cases like receiving verification codes or customer messages. This is “email for the agent,” not “AI operating the owner’s email.”

## Distinction

| Email account role         | Meaning                                    | Risk posture                                       |
| -------------------------- | ------------------------------------------ | -------------------------------------------------- |
| `owner_mailbox`            | User’s personal/work mailbox               | High privacy risk; selected-scope access preferred |
| `agent_inbox`              | Dedicated inbox owned by an agent/workflow | Operational identity risk; still gated             |
| `shared_workspace_mailbox` | Team/business mailbox                      | Multi-party/compliance risk                        |
| `customer_intake_inbox`    | Inbound customer/lead mailbox              | External communication lane; prompt-injection risk |
| `notification_inbox`       | System alerts/CI/CD notifications          | System/development lane                            |

## Consequences

- Dedicated agent inboxes should be modeled as communication connectors/accounts with explicit account roles.
- Agent-owned inboxes may support more autonomous workflows than owner mailbox access, but still require capability packs, task grants, audit, and policy.
- Using an agent inbox for account signups, OTPs, newsletters, or customer intake should be treated as a distinct workflow, not as owner email access.
- Sending from an agent-owned inbox may still have external visibility and reputation/deliverability risk.

## Would change if

All email usage is intentionally limited to owner-selected personal mailbox threads. Current direction should allow agent-owned inboxes later.

---

# D-023 — OpenSpine is the substrate; Lyra is a product built on it

## Decision

Rename the reusable substrate/framework to **OpenSpine**. Treat **Lyra** as a governed personal assistant product built on OpenSpine.

## Rationale

“Lyra agent” implies a single assistant. The architecture is actually a backbone for composing agents, tools, workflows, memory, connectors, routes, and capabilities. OpenSpine better expresses the reusable substrate.

## Positioning

> OpenClaw gives an assistant claws. OpenSpine gives it a backbone.

Longer framing:

> Lyra is a governed personal assistant built on OpenSpine, a framework for safely composing agents, tools, workflows, memory, and connectors as capability grows.

## Consequences

- PRD/specs should distinguish OpenSpine core from Lyra product.
- OpenSpine owns runtime concepts: event envelope, identity, route, authority composition, task grant, gate, connectors, audit, artifact lifecycle.
- Lyra owns the first product experience: Telegram owner control, assistant behavior, selected-thread email drafting, user-facing setup.
- Future products could be built on OpenSpine without inheriting Lyra’s exact assistant UX.

## Would change if

The project intentionally narrows back to a single personal assistant app rather than a reusable agent runtime.

---

# D-024 — OpenSpec is the development/change-management layer, not the runtime

## Decision

Use OpenSpec-style spec-driven development to develop OpenSpine, but do not confuse OpenSpec with OpenSpine’s runtime architecture.

## Rationale

OpenSpec is useful for organizing proposals, design artifacts, tasks, and delta specs. OpenSpine is the runtime substrate that executes events, routes, capabilities, agents, tools, memory, connectors, and audit.

## Mapping

| OpenSpec concept         | OpenSpine relevance                                                                       |
| ------------------------ | ----------------------------------------------------------------------------------------- |
| specs as source of truth | OpenSpine core behavior specs                                                             |
| changes folder           | proposed substrate/product changes                                                        |
| proposal/design/tasks    | implementation planning artifacts                                                         |
| custom schemas           | possible OpenSpine-specific workflow: research → decision → spec → tests → implementation |
| archive                  | merge accepted behavior into canonical specs                                              |

## Consequences

- Use OpenSpec to create implementation slices such as “telegram-owner-control-slice.”
- Keep OpenSpine runtime artifacts separate from OpenSpec development artifacts.
- OpenSpine may later borrow OpenSpec-like artifact lifecycle ideas internally, but runtime authority must remain task-grant/gate based.

## Would change if

Another project management/spec framework proves more suitable. The OpenSpine architecture does not depend on OpenSpec.

---

# D-025 — Rust/Tokio substrate: storage, audit chain, and secrets handling

## Decision

OpenSpine's substrate is implemented in Rust on the Tokio async runtime, as a workspace of five crates (`openspine-schemas`, `openspine-authority`, `openspine-gate`, `openspine-kernel`, `openspine-shell`). Storage is SQLite via `rusqlite` (bundled). The audit log is append-only and hash-chained: `hash = sha256(prev_hash || canonical_json(meta))`, genesis `prev_hash = "sha256:" + 64×"0"`. Bootstrap secrets (bot token, artifact key, provider API keys) are read from environment variables; OAuth tokens are encrypted at rest under `data/credentials/` with AES-256-GCM, keyed by `OPENSPINE_ARTIFACT_KEY`.

## Rationale

Rust's ownership model and strong typing suit a security-load-bearing authority/gate boundary: merge-rule and precedence bugs there are security bugs (D-004, D-007). Tokio is the standard async runtime for the HTTP/bot-polling/provider-call workload. SQLite keeps the reference deployment single-binary-friendly and matches D-020 (deployment-agnostic core) — no external database service required. A hash chain gives tamper-evidence for D-012's audit-integrity goal without a external ledger dependency. Env-var bootstrap secrets are an explicitly documented deferral, not a final answer — D-014's secret-intake flow remains a future change; this decision only unblocks phases 1–3.

## Consequences

- New Rust code must pass `cargo fmt --check`, `clippy -D warnings`, and a 500-line-per-file gate (`scripts/check-file-sizes.sh`), mirroring the house Rust convention.
- `openspine audit verify` walks the chain and exits non-zero on any break; this is a first-class CLI subcommand, not a debug tool.
- OAuth/API secrets never appear in plaintext on disk outside the bootstrap env vars documented in the README threat notes.

## Would change if

Multi-node/horizontally-scaled OpenSpine deployments require a shared database; SQLite would then be replaced (Postgres) behind the same storage-module trait boundary, and the secret-intake flow (D-014) lands, retiring the env-var bootstrap path.

---

# D-026 — Shell containment via a `SandboxDriver` trait (Process dev-only / Docker)

## Decision

Shell containment (D-005) is implemented behind a `SandboxDriver` trait with two implementations for phases 0–3: `ProcessDriver` (spawned child process, scrubbed env) and `DockerDriver` (per-task container on an internal-only network, no secrets, non-root, read-only rootfs). The kernel refuses to route `external_communication` events when the active driver is `process`, unless `unsafe_allow_uncontained_private_data: true` is explicitly set in `openspine.yaml`.

## Rationale

`ProcessDriver` is needed for fast local development but gives no real exfiltration containment (D-005's "no unmediated exfiltration paths" requires network isolation, which a bare child process does not have). Rather than banning it outright and slowing early development, the kernel gates it: uncontained private-data flows require an explicit, auditable opt-in flag. `DockerDriver` is the first driver that actually satisfies D-005 for real user data. A future bubblewrap/nsjail driver for mass-parallel-agent scaling is explicitly out of scope for phases 0–3.

## Trade-offs

| Option | Benefit | Risk |
| --- | --- | --- |
| `ProcessDriver` only | Fastest dev loop | No real containment; unsafe for private data |
| `DockerDriver` only from day one | Real containment from the start | Slower local dev loop, requires Docker everywhere |
| Trait + explicit unsafe flag (chosen) | Fast dev loop, safe default, single code path | Requires discipline to never flip the flag against real accounts |

## Consequences

- `openspine.yaml` carries `sandbox.driver: process|docker` and `unsafe_allow_uncontained_private_data: false` by default.
- Containment tests (Step 4 tasks.md) assert: no `OPENSPINE_*`/provider secrets in the spawned shell's env, and arbitrary egress fails under `DockerDriver`.

## Would change if

A bubblewrap/nsjail driver is built for mass-parallel-agent scaling (explicitly deferred past phase 3), or a managed/cloud sandbox provider is adopted instead of self-hosted Docker.

---

# D-027 — Multi-provider model gateway with per-provider auth mode

## Decision

The model gateway (D-010) supports multiple providers via a `ProviderClient` trait, each configured with an auth mode of `api_key` (env-var-sourced) or `oauth` (generic PKCE login flow). Phase 1 ships `anthropic` (api_key env, with a PKCE OAuth login path) and `openai_compat` (base_url + api_key, config-only). The gateway owns the final provider call; the shell never sees provider credentials.

## Rationale

A single hard-coded provider would block later product decisions and make the "model calls go through the gateway" boundary (D-010) untestable in isolation. Supporting both `api_key` and `oauth` from the start means adding a provider is a config + trait-impl change, not an architecture change. `openai_compat` covers the wide set of OpenAI-API-compatible providers (local and hosted) without bespoke code per vendor.

## Consequences

- `providers.yaml` entries are `{ id, kind, base_url?, auth }`; the gateway resolves auth mode per provider at call time.
- OAuth tokens for a provider are encrypted at `data/credentials/<id>.json.enc`, same mechanism as D-025.
- If Anthropic's OAuth endpoints prove unusable for third-party PKCE, Anthropic narrows to `api_key`-only and this is recorded as a follow-up amendment here rather than blocking the gateway trait design.

## Would change if

A managed/hosted OpenSpine offering centralizes provider credentials server-side rather than per-self-hosted-instance.

---

# D-028 — Canonical artifact format is YAML+serde; digests are `sha256:` over canonical JSON

## Decision

Declarative artifacts (routes, agent manifests, capability packs, workflows, policies, prompt templates) are authored as YAML files on disk. `serde` structs with `#[serde(deny_unknown_fields)]` and an explicit `schema_version: u32` field are the canonical typed schema — this is the whole validation engine; no separate JSON-Schema layer. Canonical JSON (recursive key-sort, no insignificant whitespace, UTF-8) is used only as the digest pre-image: `Digest = sha256:<64 lowercase hex>`. Artifact versions are monotonically increasing `v<N>` per artifact id; `authority_sources` entries are `<kind>:<id>:v<N>` exactly as in the PRD examples; audit rows additionally record the content digest.

## Rationale

YAML is the readable, hand-editable format the PRD's own artifact examples already use (§6/§10/§11/§12); a human (or an agent proposing an artifact per D-013) should be able to read and review a route or capability pack without tooling. `deny_unknown_fields` serde structs give strict, fail-closed validation for free at deserialization time, avoiding a second schema-description language (JSON Schema) that could drift from the Rust types it is meant to describe. Canonical JSON is only needed where byte-stability matters — digesting and approval-binding (D-011) — so it is scoped to that one function rather than becoming the storage format.

## Consequences

- `openspine-schemas::digest::canonical_json` and `digest_of` are the two load-bearing digest functions; every other crate calls through them rather than re-implementing hashing.
- Any unrecognized field in an artifact YAML is a hard parse error, not a silently ignored one.
- Closes O-005 (canonical artifact format) and O-008 (digest/version representation).

## Would change if

A future multi-tenant/marketplace artifact-distribution feature needs a machine-generated schema description (e.g. for third-party artifact authoring tools); JSON Schema could then be generated *from* the serde types as a derived artifact, not a parallel source of truth.

---

# D-029 — Gmail OAuth scopes: `readonly` + `compose`, send hard-denied at the gate

## Decision

The Gmail connector requests `https://www.googleapis.com/auth/gmail.readonly` and `https://www.googleapis.com/auth/gmail.compose`. There is no draft-only Google scope — `gmail.compose` technically permits send at the OAuth layer. This is mitigated, not avoided: the OAuth token lives in the kernel only (D-010, shell never sees it), and `email.send` is a hard `Deny` in `gate()` regardless of grant or approval state (consistent with D-015).

## Rationale

Google does not offer a scope that allows draft creation without also nominally allowing send; refusing to integrate over this would block Phase 2/3 entirely. The actual security boundary is not the OAuth scope, it is that the token never leaves the kernel and the gate never permits the `email.send` action id to reach the connector, so no code path can invoke Gmail's send endpoint even though the token could authorize it.

## Consequences

- README threat notes document this scope/action-boundary distinction explicitly, so a future reviewer does not "fix" the send hard-deny thinking it is redundant with scope choice.
- Adding a real send capability later requires a new decision entry, not just removing the gate deny (D-015 stays in force until explicitly revisited).

## Would change if

Google ships a send-excluded compose scope, or a later phase (post D-015 revisit) intentionally adds guarded send with its own approval ceremony.

---

# D-030 — Telegram carries the entire owner-control UX for phases 1–3

## Decision

Telegram is the first and, through phase 3, the only owner-control channel (closes O-001), built before Gmail integration per the already-fixed change sequence (closes O-002: changes 4 → 5). No separate web UI is built for phases 1–3 (closes O-006): chat, status, the email thread-selection flow, draft preview, and inline-button approve/reject are all kernel-owned Telegram flows. Long-polling (`teloxide`) is used rather than webhooks.

## Rationale

D-002 already established Telegram as the practical first owner channel. Extending that to "carries everything through phase 3" avoids building and securing a second UI surface (web) before the core authority/gate/containment substrate is proven — PRD §15's "approved owner-control selection flow" is explicitly satisfiable via chat + inline buttons, so a picker UI is not a hard requirement. Long-polling avoids needing a public HTTPS endpoint/TLS cert during early development; the design explicitly permits it and treats webhooks as a later change.

## Consequences

- `/email` thread selection, draft preview, and `[Approve]`/`[Reject]` are all Telegram inline-keyboard flows, not a web picker.
- A future web UI (if built) is additive, not a phase 1–3 blocker.
- Closes O-001, O-002, O-006.

## Would change if

A later phase needs owner interaction Telegram cannot express well (e.g. large structured review, bulk approvals) — a web UI would then be added as an additional channel, not a replacement.

---

# D-031 — Docker Compose is the first reference deployment target

## Decision

The first reference deployment is a Docker Compose stack (kernel + shell services), runnable identically on a Linux server or macOS dev via Docker Desktop. Railway (or any other managed one-click target) remains deferred, consistent with D-020's deployment-agnostic core.

## Rationale

Compose is the natural fit for `DockerDriver` (D-026): the same per-task shell-container mechanism used for containment is expressed as compose services, so there is one deployment story instead of a "dev mode" and a "real mode." It gives a reproducible, inspectable target for the containment tests (docker inspect the spawned shell, verify network mode/user/env) without committing to a specific cloud platform.

## Consequences

- `compose.yaml`, `Dockerfile.kernel`, `Dockerfile.shell` are first-class repo artifacts, exercised by CI-adjacent manual checks.
- Railway/other managed targets remain a future onboarding path (D-020), layered on top of the same containers.
- Closes O-007.

## Would change if

Product adoption data shows self-hosting Docker Compose is too high a barrier for target users, prompting a managed hosted offering as the primary path.

---

# D-032 — Kernel↔shell transport is HTTP/JSON with a per-task bearer token

## Decision

The kernel exposes an HTTP/1.1 + JSON API (`axum`); the shell calls it via `reqwest` with `Authorization: Bearer <task_token>`. The task token is a per-task random 32-byte secret minted at grant issuance (D-007) — it identifies the shell's task to the kernel, it is not a connector/provider secret. Transport is a Unix domain socket (`data/kernel.sock`) under `ProcessDriver`, or `http://kernel:7777` on the compose-internal network under `DockerDriver`.

## Rationale

HTTP/JSON keeps the kernel API introspectable and easy to test (including from `wiremock`-style fixtures) without inventing a bespoke RPC protocol. Scoping the bearer token to one task (rather than one long-lived shell credential) means a compromised/leaked shell process only ever holds authority for its own already-granted, time-boxed task — consistent with D-007's "task grant is the only live authority object" principle applied to the transport layer itself.

## Consequences

- Every kernel API request without a valid, unexpired task token gets `403` + an audit entry.
- Under `DockerDriver` the kernel API is reachable only on the compose-internal network — no host port publishing needed for shell↔kernel traffic.

## Would change if

A future multi-shell-per-task or streaming-response requirement pushes toward gRPC/WebSocket; the bearer-token-per-task authority model would carry over unchanged.

---

# D-033 — Action identifiers are exact-match dotted strings; unverified senders are audited and ignored

## Decision

Action identifiers are dotted strings exactly as written in the PRD (e.g. `email.send`, `telegram.reply:owner_channel`, `email.read_thread:selected_no_attachments`), wrapped in a newtype `ActionId(String)`. Matching in `gate()` and task grants is exact-string-only — the `:qualifier` suffix is part of the identifier; there is no wildcard or prefix semantics in phases 0–3. Separately: a Telegram message from a sender who does not match the configured owner id is logged, audited, and ignored — no reply is sent.

## Rationale

Exact-match action ids keep authority composition (D-007, D-008) simple to reason about and test: a grant either lists an action id or it does not, with no pattern-matching engine that could be a source of subtle over-broad grants. This can be revisited once real usage shows the id space needs hierarchy, but starting narrow is the safer default per D-013 (easy behavior, hard authority). For non-owner senders, PRD §19's incident table allows "ignore or low-authority triage"; ignoring is the conservative choice for phase 1 — no reply avoids acknowledging the bot's existence/behavior to an unverified party, while the audit trail still records the event for later review.

## Consequences

- Adding a new qualified variant of an action (e.g. a new `:scope` suffix) requires updating every grant/pack that should carry it — an explicit, reviewable change, not an implicit widening.
- `audit_log` contains a row for every non-owner Telegram message even though the owner never sees a reply.

## Would change if

Action-id volume grows large enough that hierarchical/prefix matching is needed for maintainability; that would be its own decision with its own precedence-rule analysis, not a quiet extension of exact-match. Non-owner handling would change if a future low-authority triage path (e.g. routing to a support/log-only agent) is explicitly designed and audited.

---

# D-034 — `email.create_draft` is the one canonical action id; the qualified PRD §10.2 spelling is dropped

## Decision

The only action id for creating a Gmail draft is the bare `email.create_draft` (matching PRD §11.2's pack and §12.2's task-grant example). PRD §10.2's `email_reply_drafter.designed_tools` entry `email.create_draft:after_payload_approval` is treated as a PRD-internal inconsistency and is **not** used anywhere in the implementation; the Lyra fixture (`artifacts/lyra/agents/email_reply_drafter.yaml`) transcribes the bare id instead, with an inline note.

## Rationale

D-033 makes action ids exact-match strings with no wildcard/prefix semantics — two different spellings of "the same" action are, to `gate()` and to authority composition, two unrelated actions. Authority composition (Step 2, `implement-authority-composition`) unions an agent's `designed_tools` into the candidate-allow set. Keeping the qualified spelling would put `email.create_draft:after_payload_approval` into `allowed_actions` as a **plain allow** — with no corresponding entry in any pack's `approval_required` list — silently bypassing the digest-bound approval gate (D-011) that the qualifier's own name claims to require. PRD §12.2's task grant example is unambiguous ground truth: no `create_draft` variant ever appears in `allowed_actions`, only in `approval_required_actions`, and only as the bare id. Discovered while implementing Step 2's compose_authority merge logic; caught before any code shipped that could have made the bug live.

## Consequences

- `email.create_draft` is the only spelling used in Lyra fixtures, `openspine-authority` tests, and (later) `openspine-kernel`'s Gmail draft connector action.
- `openspine-authority`'s test suite includes a regression test asserting the composed selected-thread-email task grant exactly matches PRD §12.2: no `create_draft` variant in `allowed_actions`, exactly `email.create_draft` in `approval_required_actions`.

## Would change if

A future action-id scheme intentionally adds qualified variants with their own independent approval requirements (i.e. `:qualifier` becomes meaningful for approval routing, not just descriptive) — that would be its own decision, not a quiet exception carved out for this one action.

---
# D-035 — Kernel advertises a separate `advertise_endpoint` from its `bind_addr`; no Unix-domain-socket transport for `ProcessDriver`

## Decision

`openspine.yaml`'s `kernel` block gets a second, optional field,
`advertise_endpoint`, distinct from `bind_addr`. `bind_addr` is what the
kernel's HTTP listener binds to; `advertise_endpoint` (default: derived as
`http://<bind_addr>`) is what the kernel tells the shell to connect to via
the `KERNEL_ENDPOINT` environment variable. Under `DockerDriver`, an
operator sets `bind_addr: 0.0.0.0:7777` (so the shell's container can reach
it) and `advertise_endpoint: http://kernel:7777` (the compose service DNS
name — `0.0.0.0` is not a connectable destination). Separately, this
decision narrows D-032's stated transport for `ProcessDriver`: the kernel
listens on plain TCP loopback (`127.0.0.1:<port>`) under `ProcessDriver`
too, **not** the Unix domain socket (`data/kernel.sock`) D-032 originally
specified.

## Rationale

The `advertise_endpoint` split fixes a real reachability bug: `KERNEL_ENDPOINT
= http://{bind_addr}` breaks the moment `bind_addr` is a wildcard address,
which it must be for the kernel to be reachable from a Docker container on
the compose-internal network at all. This is a pure bugfix with no
downside — the field is optional and defaults to today's loopback-only
behavior.

The `ProcessDriver` transport narrowing is a real, deliberate walk-back of
D-032's literal text, not an oversight. Implementing a Unix-domain-socket
HTTP client for the shell would need either a new dependency (`reqwest`
has no built-in UDS transport; the closest crates — e.g. `hyperlocal` —
are not in the approved dependency set and the no-new-deps convention
requires justification this doesn't clear) or a bespoke `hyper`-based UDS
connector, disproportionate effort for the security benefit actually
gained: `ProcessDriver` is already documented as dev/testing-only with "no
network or filesystem isolation beyond the OS user boundary" (`sandbox.rs`).
A loopback-only TCP bind is not reachable from any other host and offers
no materially different exposure than a UDS for a single local dev
process — the marginal gain of Unix-file-permission-scoped access control
over "only this machine can connect" doesn't justify the cost here.
`DockerDriver` (the production path) already gets the real isolation
guarantee D-032 cares about — an internal-only compose network with no
host port published for shell↔kernel traffic — via TCP, unchanged.

## Consequences

- `openspine.yaml`'s `kernel.advertise_endpoint` is optional
  (`#[serde(default)]`); omitting it preserves today's behavior exactly.
- `ProcessDriver` deployments have no OS-level access control on the
  kernel API beyond "who can reach 127.0.0.1 on this host" — acceptable
  per its existing "dev/testing only" flag, not a new exposure.
- `docs/kernel-http-contract.md` documents both the wire contract and this
  bind-vs-advertise distinction.

## Would change if

A same-host multi-tenant deployment of `ProcessDriver` becomes a real
target (several unrelated owners' kernels on one box, where OS-file-
permission-scoped UDS access control would matter) — that would justify
revisiting the no-new-deps tradeoff above with fresh rationale, not a
silent reversal of this one.

---



## Open Decision Questions — CLOSED (see linked decisions)

| ID    | Question                                                    | Resolution |
| ----- | ----------------------------------------------------------- | ---------- |
| O-001 | Is Telegram definitely the first owner control channel?     | Closed by D-030: yes, Telegram (`teloxide`, long-polling) is the sole owner-control channel through phase 3. |
| O-002 | Should Gmail OAuth be added before or after Telegram setup? | Closed by D-030: Telegram first — the applied change sequence fixes changes 4 (Telegram) → 5 (Gmail). |
| O-003 | What is the exact shell containment implementation?         | Closed by D-026: `SandboxDriver` trait, `ProcessDriver` (dev-only, unsafe-flagged) / `DockerDriver`. |
| O-004 | What is the first model provider policy?                    | Closed by D-027: multi-provider gateway, per-provider `api_key`/`oauth` auth; ships `anthropic` + `openai_compat`. |
| O-005 | What is the canonical artifact format?                      | Closed by D-028: YAML on disk, `deny_unknown_fields` serde structs as the schema, canonical JSON only as the digest pre-image. |
| O-006 | How much UI is needed for phase 1?                          | Closed by D-030: none — Telegram carries chat, status, selection, preview, and inline approval for phases 1–3. |
| O-007 | What is the first deploy target?                            | Closed by D-031: Docker Compose (kernel + shell services), macOS-dev-compatible via Docker Desktop. |
| O-008 | How are artifact digests and versions represented?          | Closed by D-028: `sha256:<64 hex>` over canonical JSON; versions `v<N>`; `authority_sources` as `<kind>:<id>:v<N>`. |

---

## Research / Reference Backlog

Potential areas to research before implementation decisions:

1. Gmail OAuth scopes for read selected thread, create draft, and whether send authority is bundled.
2. Telegram bot security model, owner user ID verification, and webhook vs polling trade-offs.
3. Practical Linux containment options for shell worker: Docker, rootless container, bubblewrap, firejail, systemd-run, gVisor, nsjail.
4. Secret intake UX patterns for self-hosted agents.
5. Audit hash-chain and checkpoint designs suitable for local-first/self-hosted systems.
6. Existing systems to compare: OpenClaw, Hermes, LangGraph, n8n, Dagger, Temporal/DBOS, OpenWebUI pipelines, Claude Code agents/skills, MCP security patterns.
7. Prompt-injection mitigation for email and tool-output contexts.
8. Artifact lifecycle and schema validation approaches.

---

## Change Log

| Date       | Change                                                                |
| ---------- | --------------------------------------------------------------------- |
| 2026-04-26 | Initial companion decisions log created from PRD v4–v8 review thread. |
| 2026-07-02 | Added D-025–D-033 (Rust/Tokio stack, containment driver, model-gateway auth, artifact format/digests, Gmail scopes, Telegram-only UX, deploy target, transport, action-id/non-owner handling); closed O-001–O-008 (Step 0 of the implementation plan). |
| 2026-07-02 | Added D-034: normalized the email-drafter's create-draft action id to the bare `email.create_draft`, dropping PRD §10.2's qualified spelling to close a would-be approval-bypass gap discovered while implementing Step 2 (`implement-authority-composition`). |
| 2026-07-02 | Added D-035: split `kernel.advertise_endpoint` from `bind_addr` (fixes Docker-compose shell↔kernel reachability) and narrowed D-032's `ProcessDriver` transport to plain loopback TCP instead of a Unix domain socket, discovered while implementing Step 4 (`implement-telegram-owner-control-slice`). |
