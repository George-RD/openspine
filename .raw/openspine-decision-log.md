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
| D-035 | Kernel `advertise_endpoint` split from `bind_addr`; `ProcessDriver` uses plain TCP loopback | Accepted |
| D-036 | Phase-2 selection trigger is a kernel-recognized `/draft <thread_id>` command | Accepted |
| D-037 | Gmail OAuth token exchange is a plain refresh-token POST; no `oauth2` crate | Accepted |
| D-038 | `resolve_owner_identity`'s `channel_trust` is caller-supplied, not hardcoded | Accepted |
| D-039 | Draft-approval channel is a Telegram inline button (`callback_query`)     | Accepted |
| D-040 | Pending `ActionRequest`s are persisted in a new `action_requests` table   | Accepted |
| D-041 | `email.create_draft` digests: payload `{subject, body}`, target `{thread_id, connector, account_role, recipients}` | Accepted |
| D-042 | Reply recipient is kernel-derived (newest non-owner sender), never shell-supplied | Accepted |
| D-043 | `lyra.ui.preview` is extended to propose the exact reviewed draft + approval button | Accepted |
| D-044 | Approved draft creation dispatches kernel-side; no new shell spawn        | Accepted |
| D-045 | WYSIWYS: a truncated preview refuses an approval button rather than splitting the message | Accepted |
| D-046 | Grant budgets are enforced kernel-dispatch-side; the artifact budget counts only shell-initiated puts | Accepted |
| D-047 | Task tokens are hashed at rest; expired grants are swept | Accepted |
| D-048 | `artifact.activate` is the single canonical activation action id; all runtime proposals require uniform owner approval; prompt templates are not proposable | Accepted |
| D-049 | Capability specs are backfilled for subsystems implemented inside earlier slices | Accepted |
| D-050 | `max_model_calls` is enforced with an atomic upsert, not a count-then-compare | Accepted |
| D-051 | The agent-OS canon (AD-001..153) is decomposed into a dependency-edged change sequence; the stale later-changes placeholders are superseded or subsumed | Accepted |
| D-053 | Kernel extension points are compiled-in registries; a curated canonical `ActionCatalog` makes unknown action ids a hard composition error and a structured `UnknownAction` gate denial distinct from `NotGranted` | Accepted |
| D-054 | Pipeline stages are a typed compiled-in sequence the driver executes; lanes are compiled-in `LaneSpec` data records with a single-stage hook contract; gate is a distributed runtime stage outside the driver prefix | Accepted |
| D-055 | Gate trusted paths are hardened: carve-outs are enumerated catalog data; `KernelOrigin` is approval-exempt, audit-never-exempt; selection-token validation lives in pure `gate()` with dispatch-side consumption; digests are kernel-re-derived at approval-effect time | Accepted |
| D-056 | Eval-store groundwork defers AD-111 evaluator policy: only the indexed verdict-landing surface is settled — open verdict string, optional fitness/evidence/evaluator metadata; judge-independence, evaluator identity, attack-trace evidence semantics, and verdict vocabulary return to the owner with the later evaluation change | Accepted |
| D-057 | Counterparty-facing actions are an explicit kernel ActionCatalog set (v1: `email.send` only); only such denials receive the canonical deferral + escalation — internal/owner-only/unclassified actions keep typed enum outcomes | Accepted |
| D-058 | Security escalations require result-returning gated owner delivery: `action.escalated` is appended only after connector success; missing-key/gate/connector failures record `owner.notify_failed` and return structured errors; courtesy notifications may stay best-effort | Accepted |
| D-059 | Dormant thread bindings are MAC-authenticated before activation: `TaskGrant.thread_id` participates in the root-authority canonical commitment when populated (omitted when `None` for legacy-grant compatibility) | Accepted |
| D-060 | The AD-142 overlay eval gate's first-cut evaluator is a deterministic owner-control-history availability gate plus structural artifact probes; the full OQ-17 holdout replay and AD-111 prover-verifier protocol arrive with a later owner-ratified evaluator change (stays within D-056's deferral) | Accepted |
| D-061 | Model-swap golden sets use a bounded deterministic first cut: operator-owned role-bound fixtures, at least three standard plus one adversarial case, deterministic substring criteria, a 20-case cap, bounded prompts/evidence, and replay timeout capped by both five minutes and the grant's remaining expiry; attempted calls consume reserved budget | Accepted |
| D-062 | An active model swap is restorable only when the exact normalized manifest matches the latest persisted Active proposal and its digest-bound replay and judge verdicts; startup fails closed rather than silently falling back when DB provenance and overlay state disagree | Accepted |
| D-063 | Model-swap activation is a serialized, provenance-bound staged protocol: lifecycle, supersession, and activation audit commit transactionally before provider publication; `.pending` files are loader-invisible and startup either completes a digest-matching committed activation or quarantines/removes an uncommitted or tampered candidate | Accepted |
| D-064 | Connector secrets migrate once into the kernel vault; connectors resolve vault slots at call time | Accepted |
| D-065 | Provider API-key vault migration belongs to the foundation-amendment lane | Accepted |
| D-066 | Paired Gmail credentials stage until atomic validated promotion | Accepted |
| D-067 | Telegram poll offsets are namespaced by bot identity and legacy state migrates once | Accepted |
| D-068 | Authenticated API bad requests surface directly without duplicate owner notification | Accepted |
| D-069 | Kernel connector counters are the minimal observability surface until a metrics contract exists | Accepted |
| D-070 | Retryable owner notifications reference encrypted artifacts; persistence failure stays plaintext-free | Accepted |
| D-071 | External owner delivery is delivery-unknown across the send-to-receipt crash window | Accepted |
| D-072 | `/digest <ULID> [page]` is a secure lossless pagination substrate; presentation remains deferred | Accepted |
| D-073 | Durable workflow steps persist intent before effect; recovery replays recorded outcomes and fails closed on receiptless pending effects | Accepted |
| D-074 | Workflow timers are kernel-fired at most once via trusted-clock atomic claims | Accepted |
| D-075 | The daily spend kill switch accounts for every model and connector call; breach pauses only non-immediate lanes | Accepted |
| D-076 | Spend caps are required finite configuration; disabling requires an explicit large cap | Accepted |

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

# D-036 — Phase-2 thread selection is a kernel-recognized `/draft <thread_id>` command, not free-form NLU or a shell-supplied id

## Decision

The "trusted owner selection path" PRD §15/§21.1 requires is, for Phases 1–3, a single structured Telegram command the **kernel itself** recognizes before any shell/agent ever sees the message: `/draft <gmail_thread_id>`. Recognizing it is a pure function (`telegram::parse_draft_command`) that runs in the same place `verify_update` already runs — strictly before routing. A match short-circuits the normal `owner_telegram_main_assistant` route entirely and enters a separate path (`pipeline::handle_thread_selection`) that verifies the thread exists via a live Gmail call, mints the `SelectionToken` itself, builds the `email.thread.selected` envelope, and composes authority for `email_reply_drafter` as a new task.

The event envelope's `verification_method` is `kernel_ui_selection` (the kernel is the "UI" that produced the selection); the selection token's own `verification_method` is `approved_owner_control_selection` (the trigger arrived over the already-verified owner-control Telegram channel) — PRD §15 offers both terms without saying which applies to which record, and this is the disambiguation.

## Rationale

`tasks.md` for `implement-selected-thread-email-preview-slice` explicitly permits "the trusted owner selection path **or a controlled test stub**." A real thread-browsing picker (list threads, natural-language "the one from Alex about the invoice") is a genuine NLU/UX problem orthogonal to this slice's actual subject — the security boundary around *using* a selection (single-use token, shell cannot mint or forge one, untrusted-data wrapping downstream). A narrow, kernel-recognized command satisfies the letter and spirit of the PRD rule ("shell cannot mint or alter... issued only by kernel-owned UI, verified picker, or approved owner-control selection flow") without fabricating a features-complete picker nobody asked this slice to build. The real cost is UX: the owner must already know a Gmail thread id (visible in the Gmail web UI's URL) — materially worse than a picker, and documented as follow-up scope in `docs/gmail-setup.md`, not built here.

## Consequences

- A real picker/NLU-based selection (browsing recent threads, subject search) is explicit future work — not fabricated ahead of a real need.
- `telegram::parse_draft_command` is the entire trust boundary for "did the owner actually select this" on the Telegram side; it is unit-tested exhaustively (exact-prefix match, no fuzzy matching, empty-id rejection).
- No additional Gmail scope or thread-listing endpoint exists solely to support this command.

## Would change if

A kernel-owned UI/picker (PRD's other named option) is built — this decision's command-based stub is superseded, not reversed; the selection-token/gate architecture underneath is unaffected either way.

---

# D-037 — Gmail OAuth via a plain refresh-token POST (no `oauth2` crate); `base64` promoted from transitive to direct dependency

## Decision

`GmailConnector` (Step 5) exchanges a long-lived refresh token for short-lived access tokens with one `POST https://oauth2.googleapis.com/token` (`grant_type=refresh_token`), implemented as a plain `reqwest` form POST — the same "raw HTTP client, no vendor SDK" pattern `model_gateway::providers::ProviderClient` already uses for Anthropic/OpenAI. The refresh token itself is supplied via an env var (`OPENSPINE_GMAIL_REFRESH_TOKEN`, named by `openspine.yaml`'s `gmail.refresh_token_env`), obtained once by a human manually completing Google's OAuth consent screen outside this codebase (documented in `docs/gmail-setup.md`) for the scopes D-029 already settled (`gmail.readonly` + `gmail.compose`) — this decision covers only how the token exchange itself is implemented, not which scopes are requested.

Decoding Gmail API message bodies (`body.data`, base64url-encoded) uses the `base64` crate rather than a hand-rolled decoder: `cargo metadata` already resolves `base64 0.22.1` transitively (pulled in by the existing `reqwest`/`rustls` dependency tree), so adding it as a direct `openspine-kernel` dependency introduces zero new transitive dependencies — it only promotes a crate already vetted-by-inclusion to a direct, visible one. This is a better fit for the no-new-deps convention's intent than hand-rolling base64, which the convention reserves for surfaces with no acceptable existing option.

## Rationale

The `oauth2` crate (named as a candidate workspace dependency in the original implementation plan) is built for interactive authorization-code/PKCE flows with redirect handling — machinery this slice never exercises, since the human-in-the-loop consent step happens once, outside the kernel process, and the kernel only ever performs the mechanically simple, extremely stable refresh-token grant. Pulling in a general OAuth client crate for one documented endpoint would be dependency weight against a capability this slice doesn't use, and is exactly the case the no-new-deps convention asks to justify against a smaller alternative first. If a future phase needs the full authorization-code/PKCE flow (e.g. a self-serve "connect your Gmail" setup wizard), `oauth2` becomes justified then — this decision does not preclude adding it, it only declines to add it ahead of a real caller. The refresh-token env-var intake is the same documented secret-intake shortcut as the bot token/artifact key (D-014's deferral) — a richer secret-intake flow remains future work.

## Consequences

- `crates/openspine-kernel/Cargo.toml` gains `base64.workspace = true` (direct); the `oauth2 = "5.0.0"` line Step 0's bootstrap pre-declared (for a not-yet-built provider-OAuth-login feature, per `ProviderAuth::Oauth`'s doc comment — never `oauth2::`-imported by any code) is removed rather than left as dead weight. Re-adding it is cheap once a real caller exists.
- `docs/gmail-setup.md` documents the one-time manual OAuth consent step and the two scopes requested.

## Would change if

A future phase needs the interactive consent flow itself run from inside the kernel (not just a human completing it once) — that is when `oauth2` earns its place.

---

# D-038 — `resolve_owner_identity`'s `channel_trust` is caller-supplied, not hardcoded

## Decision

`resolve_owner_identity(envelope, channel_trust)` takes `channel_trust` as a parameter rather than hardcoding `ChannelTrust::VerifiedOwnerChannel` internally. Ordinary owner-control chat (`handle_owner_update`) passes `VerifiedOwnerChannel`; the `/draft` thread-selection flow (`handle_thread_selection`) passes the stronger `OwnerDevice` tier, matching the PRD's own route fixtures (`owner_telegram_main_assistant.yaml` requires `verified_owner_channel`; `owner_email_selected_thread.yaml` requires `owner_device`).

## Rationale

Both pipelines share the identical underlying signal — a Telegram sender-id match plus a private-chat check (Phase 1/2 has no separate device-attestation mechanism) — so there is no *stronger proof* backing `OwnerDevice` today. The distinction exists because the PRD's fixtures deliberately require a higher trust tier for the flow that triggers external-communication authority (reading a private mailbox, drafting a reply) than for ordinary conversational chat. Hardcoding one `ChannelTrust` value inside `resolve_owner_identity` would either force the selection flow down to the weaker tier (silently under-matching the PRD's own fixture) or force ordinary chat up to the stronger one (misrepresenting what was actually verified).

## Consequences

- `resolve_owner_identity` stays a thin, honest mapping — it reports the trust tier the caller asserts, not one it invents.
- The distinction is a deliberate design choice, not an inconsistency: it is documented at the call site, not silently smoothed over.

## Would change if

A future phase adds real device attestation (an actual second factor distinguishing "this specific device" from "this Telegram account"), at which point `OwnerDevice` would carry genuine additional evidence over `VerifiedOwnerChannel` rather than sharing its verification method.

---

# D-039 — Draft-approval channel is a Telegram inline button (`callback_query`), not a text command

## Decision

Step 6's owner approval of a drafted Gmail reply uses a Telegram inline keyboard button ("Approve"), not a `/approve <id>` text command. `ApprovalRecord.approval_channel` records `"telegram_inline"` (the value already anticipated by `approval.rs`'s doc comment when the schema was defined in Step 3).

## Rationale

`/draft <thread_id>` (D-036) is a text command because the owner is *naming* something (a thread id they already know from their own Gmail client) — free text is the natural input. Draft approval is a *yes/no* decision on content the kernel already fully controls and already sent to the owner as `lyra.ui.preview`; a tap is a strictly better UX than typing a ULID back, and only a tap-based flow avoids the owner needing to transcribe an id imprecisely (a mistyped id must fail closed, whereas a button's `callback_data` is exact by construction).

## Trade-offs

| Option | Benefit | Risk |
| --- | --- | --- |
| `/approve <id>` text command | Reuses the existing text-command parsing path (`parse_draft_command`'s pattern); no new Telegram API surface | Owner must copy/type a ULID exactly; typos fail silently-to-deny with no obvious cause |
| Inline button (`callback_query`) | Exact `callback_data` binding, no transcription; matches the schema's own anticipated value | New Telegram update kind (`callback_query`) must be polled, verified, and `answerCallbackQuery`'d |

## Consequences

- `TelegramUpdate` gains an `Option<CallbackQueryUpdate>` field; `verify_update` gains an owner-callback branch with the same sender-id + private-chat verification guarantee as text messages (D-036's "entire trust boundary" principle applies identically here).
- `TelegramConnector` gains `send_reply_with_approval_button` (attaches the inline keyboard) and `answer_callback_query` (stops the client's loading spinner; best-effort, never blocks the approval decision itself on its success).

## Would change if

A future channel (WhatsApp, a native app) becomes the primary owner-control surface and does not support inline buttons equivalently — at that point the approval-channel abstraction (already a free-form `String` on `ApprovalRecord`) accommodates a new channel-specific mechanism without a schema change.

---

# D-040 — Pending (pre-approval) `ActionRequest`s are persisted in a new `action_requests` table

## Decision

`openspine_gate::gate()`'s `GateContext::approval_for_request(action_request_id)` correlates an `ApprovalRecord` back to the exact `ActionRequest` it decides — which requires the *same* `ActionRequest` (same `id`, same digests) to exist both when it is first proposed (`ApprovalRequired`) and when it is resubmitted after approval (`Allow`). The kernel persists proposed `ActionRequest`s in a new SQLite table (`action_requests`), keyed by `id`, mirroring the existing `insert_selection_token`/`find_selection_token` and `insert_approval`/`find_approval_for_request` pattern.

## Rationale

No Step 3/4/5 action was ever `approval_required` in practice (the crate's own comment: "this has no live caller yet"), so this gap — *where does the first-proposed request live between "shown to the owner" and "owner taps approve"?* — was never closed. `email.create_draft` is the first action that actually needs it.

## Consequences

- No separate expiry field on the persisted row: usefulness is already bounded by the owning task grant's own `expires_at` (`gate()` denies an expired grant before consulting approval at all, per its existing precedence rule), so a second TTL would be redundant.
- The row is a single INSERT with no UPDATE path — an `ActionRequest` is immutable once proposed by design (mutating it after the fact would be exactly the digest-spoofing attack D-011 exists to prevent).

## Would change if

A future action needs a bounded proposal lifetime shorter than its grant's (e.g. "this specific draft proposal expires in 10 minutes even though the grant runs longer") — an explicit `expires_at` column would be added then, not speculatively now.

---

# D-041 — `email.create_draft`'s digest composition: payload = `{subject, body}`, target = `{thread_id, connector, account_role, recipients}`

## Decision

The draft's reviewed text (`subject`, `body`) is stored as a protected artifact and hashed as `ActionRequest.payload_ref.digest` (no separate payload-digest field, matching `action.rs`'s existing documented contract). The target — everything that names *where* the draft would be created and *who* it would be visible to — is hashed separately as `ActionRequest.target_digest` over canonical JSON of `{thread_id, connector, account_role, recipients}`.

## Rationale

The spec's two invalidation scenarios are deliberately distinct ("draft body changes" vs. "recipient changes") and D-011 requires *both* digests to still match for an approval to authorize a request. Folding recipients into the payload digest (or thread id into it) would conflate "what the owner read and approved" with "where it goes" — a compromised or buggy caller could then swap the target while leaving the reviewed text's digest untouched, which is exactly the "show draft A, execute draft B" failure `implement-digest-bound-draft-approval`'s proposal names as the reason this change exists.

## Consequences

- `mailbox` is represented by `account_role` (already an existing enum, `OwnerMailbox`) rather than a new free-form field — no new schema type needed.
- Any future support for a second connector/account (D-021's "email domain is broader than Gmail") only changes what populates `connector`/`account_role`, not the hashing shape.

## Would change if

A future phase adds Cc/Bcc or multiple recipients with independently variable trust (e.g. some auto-populated, some owner-added) — the recipients field would need its own internal structure, but the two-digest split (payload vs. target) stays.

---

# D-042 — Reply recipient is kernel-derived, never shell-supplied: newest non-owner sender, matched against a configured mailbox address

## Decision

At `lyra.ui.preview` dispatch time, the kernel independently re-derives the reply recipient by walking the already-fetched Gmail thread newest-message-first and taking the first message whose `From` address does not match the owner's own mailbox address (a new required `openspine.yaml` field, `gmail.mailbox_address`, documented in `docs/gmail-setup.md`). The shell is never asked for, and can never supply, a recipient.

## Rationale

A naive "last message's sender" rule breaks for an ongoing thread where the owner sent the most recent message (a self-addressed follow-up, or the owner replying to themselves while waiting on the other party) — the reply would then be addressed back to the owner's own mailbox, silently wrong. Skipping the owner's own messages when walking backward correctly finds "whoever we are actually replying to" regardless of who spoke last. This must be a kernel-derived target (D-041's target digest depends on it) exactly like `thread_id` — never something the shell's payload can influence, matching the existing "the shell has no way to name a thread directly" trust boundary from Step 5.

## Trade-offs

| Option | Benefit | Risk |
| --- | --- | --- |
| Query Gmail's `users/me/profile` for the owner's address at request time | No new config field | Extra API call and failure mode on every preview; address is static, querying it repeatedly is unnecessary work |
| Configured `gmail.mailbox_address` | One static, operator-supplied value; no extra call | Operator must set it correctly (documented, validated to be non-empty at config parse time) |

## Consequences

- `openspine.yaml`'s `gmail` block gains a required `mailbox_address` field once `gmail:` is present at all.
- If every message in the thread is from the owner's own address (no non-owner sender found), the preview dispatch fails closed with an audited denial rather than guessing — this can only happen for a thread with no correspondent, which is not a thread `email_reply_drafter` should ever be drafting a reply into.

## Would change if

A future phase supports genuinely multi-recipient replies (reply-all) or Cc — this decision only resolves *the* single reply-to address for the minimal Phase-2 slice.

---

# D-043 — `lyra.ui.preview` is extended (not duplicated) to propose the exact reviewed draft and attach the approval button

## Decision

`lyra.ui.preview`'s existing dispatch (Step 5) is the single moment that both *shows* the draft to the owner and *proposes* it for approval: it derives the target (D-042), stores the payload artifact, persists the pending `ActionRequest` (D-040) with the digests from D-041, and sends the Telegram preview message with an inline "Approve" button (D-039) whose `callback_data` names that `ActionRequest`'s id. No new action id is introduced for "propose."

## Rationale

A separate `email.propose_draft` action would let "what was shown" and "what was proposed" drift apart (e.g. a caller previews one draft but proposes another) — exactly the attack D-041 exists to prevent, reintroduced one layer up. Making the single existing preview action responsible for both closes that gap by construction: there is only ever one thing the owner could be approving, because it is the same payload/target the preview action just computed.

## Consequences

- `lyra.ui.preview`'s response contract (`{"sent": true}`) is unchanged for the shell — the shell does not need to know approval-proposal happened; that stays entirely kernel-internal, matching Step 5's dispatch-layer trust boundary.
- `email_reply_drafter`'s task grant needs `email.create_draft` marked `approval_required` (via its capability pack) even though the shell process that requested the preview will already have exited by the time the owner taps approve — this is safe because grant lookup and dispatch both happen kernel-side, not against a live shell connection (see D-044).

## Would change if

A future phase supports proposing a draft the owner has *not yet* been shown a rendered preview of (e.g. an approval queue UI) — at that point "propose" and "show" would need to split back into two actions.

---

# D-044 — Approved draft creation dispatches kernel-side; no new shell spawn

## Decision

When the owner taps "Approve," the kernel's `callback_query` handler creates the `ApprovalRecord`, persists it, then immediately re-runs `gate()` against the same persisted `ActionRequest` and the original (already-issued) `email_reply_drafter` task grant. On `Allow`, the kernel calls `GmailConnector::create_draft` directly and audits the result — no new task grant is minted and no new `openspine-shell` process is spawned.

## Rationale

`email.create_draft` is a simple, fully deterministic, non-agentic effect once approved (store a body against a thread via one Gmail API call) — it needs no model call, no untrusted-content handling beyond what was already reviewed and approved verbatim, and the original shell process that requested the preview is long gone by the time a human has read a Telegram message and tapped a button. This exactly mirrors how `/draft`'s own thread-selection flow (D-036) runs kernel-internal pipeline code rather than spawning an interactive shell to ask the kernel to ask the shell.

## Consequences

- `Store` gains a task-grant-by-id lookup (existing lookup is by `task_token` only, which the callback handler does not have — it has `task_grant_id` from the persisted `ActionRequest`).
- Draft creation is audited the same way `/draft`'s selection flow is (`authority.granted`-style rows are not applicable here since no new grant is issued; a dedicated `draft.created` / `draft.creation_failed` audit pair is added instead).

## Would change if

A future action requiring post-approval dispatch needs genuine agentic behavior (e.g. the approved step itself calls a model) — that action would need a real (short-lived, narrowly-scoped) follow-up task grant and shell spawn, not this direct-dispatch shortcut.

# D-045 — WYSIWYS: a truncated preview refuses an approval button rather than splitting the message

## Decision

`dispatch_lyra_preview` builds the full draft text first, then truncates it for Telegram. If the truncated text differs from the full text, the kernel does not call `propose_draft_creation` at all — the owner is shown a plain message with a notice that the draft is too long to approve via Telegram, and no `ActionRequest` is persisted. Only an untruncated preview may be proposed for approval.

## Rationale

Digest-bound approval (D-011/D-043) exists so a tap on "Approve" can never authorize content the owner did not review. A truncated preview breaks that guarantee at the source: the owner sees only a prefix, but `propose_draft_creation` was binding approval to the *full* body regardless. Rejected alternative: split the preview across multiple Telegram messages with the approval button on the last one — rejected because of drift risk between what is shown across parts and what a single approval record binds as a whole; refusing the button entirely is simpler and strictly safer.

## Consequences

- An owner who wants to approve a very long draft must ask the agent to shorten it — there is no in-band way to approve a truncated draft as shown.
- `dispatch_lyra_preview` and `propose_draft_creation` diverge slightly in error handling (a truncated preview is not attempted at all, versus the existing "propose failed for another reason" fallback which still shows the preview without a button).

## Would change if

A future owner-control channel supports arbitrarily long messages (removing the truncation problem entirely) or a review UX that can bind approval to a paginated/scrollable view rather than one flat message.

---

# D-046 — Grant budgets are enforced kernel-dispatch-side; the artifact budget counts only shell-initiated puts

## Decision

`GrantLimits.max_model_calls` and `GrantLimits.max_artifacts` are enforced in kernel dispatch (`post_model_generate`, `propose_draft_creation`), not inside the pure `gate()` function — the same placement precedent as selection-token single-use consumption. `max_model_calls` is checked by counting prior `"user"` conversation turns before the new one is appended, so a limit of `N` allows exactly `N` calls. `max_artifacts` is checked with one atomic SQL statement against a new `grant_counters` table, and counts only artifact blobs created *at the shell's request* (the `model.generate` payload snapshot, the draft-proposal payload) — never internal kernel bookkeeping blobs like conversation turns, which would otherwise collide with the default `max_artifacts: 20` limit under ordinary use. Separately, `notify_owner_best_effort`'s kernel-originated Telegram sends stay ungated but are now audited as `owner.notified`.

## Rationale

`gate()` is a pure decision function over an `ActionRequest`; it has no natural place to hold cross-request counters, and mixing side-effecting counter updates into it would make its precedence rules (explicit deny > approval-required > allow) harder to reason about. Dispatch already owns one other atomic, side-effecting authorization check (selection-token consumption), so extending that pattern is the smallest correct change. Counting only shell-initiated puts against `max_artifacts` keeps the default limits meaningful for ordinary conversations instead of being silently exhausted by bookkeeping.

## Consequences

- A grant that never calls `model.generate` or proposes a draft can still exceed no artifact budget from kernel-internal bookkeeping alone.
- Budget state (`grant_counters`) is swept alongside its grant (D-047) rather than living forever.

## Would change if

A future action needs to consume artifact-put budget outside `model.generate` and `propose_draft_creation` — it must call `try_count_artifact_put` itself rather than relying on a generic hook.

---

# D-047 — Task tokens are hashed at rest; expired grants are swept

## Decision

`task_grants.task_token` stores `sha256:<hex>` of the bearer token (the same raw-bytes digest helper the artifact store uses for content addressing), never the plaintext; `find_task_grant_by_token` hashes its input before the lookup. The token is also blanked before the grant is serialized into the `grant_json` column, so it cannot be recovered from either place. Expired grants (and their `grant_counters` rows) are swept — `DELETE ... WHERE expires_at < now - 24h` — at the top of every `insert_task_grant` call; no separate scheduled job exists yet.

## Rationale

A leaked or exfiltrated `data/kernel.db` file previously handed out live bearer tokens directly; hashing closes that exposure at negligible cost (tokens are 32 random bytes with no realistic timing-attack surface worth constant-time comparison at the SQL layer). The column name is left unchanged — a rename requires a full SQLite table rebuild for no behavioural benefit — with a doc comment recording the semantic change instead. A 24-hour retention window is comfortably past the ≤180s task-grant/approval TTLs already in use, so nothing live is ever at risk of being swept.

## Consequences

- Existing dev databases need no migration: plaintext rows simply stop matching once this ships, and task tokens expire in ≤180s regardless.
- Every call site that reads `.task_token` off a grant must do so on a freshly-minted, in-memory grant — never on a value loaded back from the store (verified: `grep -rn "\.task_token" crates/` finds no such site).

## Would change if

A future feature needs to recover the plaintext token from a persisted grant (e.g. a token-rotation UI) — that would need a separate, explicitly-scoped secret store, not a weakening of this hash.
---

# D-048 — `artifact.activate` is the single canonical activation action id; every runtime proposal requires uniform owner approval; prompt templates are excluded from proposable kinds

## Decision

`implement-artifact-lifecycle-slice` gives the runtime one activation action id, `artifact.activate`, mirroring D-034's precedent for `email.create_draft`: no per-kind qualified variant (`route.activate`, `workflow.activate`, `capability_pack.change`, `policy.change_proposal`, etc.) is wired to anything, even though the PRD names them — they remain candidate, unwired ids so a future change can split activation semantics per kind without a naming collision. Every proposable kind (`route | agent | workflow | pack | policy`) requires the same uniform, explicit owner approval before activation — there is no widening-detection heuristic that lets a "safe-looking" proposal skip the approval button. Prompt templates are excluded from the proposable kinds entirely: they are fixture-only until a dedicated change.

## Rationale

D-033/D-034 already established that action ids are exact-match strings with no wildcard semantics, and that a qualified spelling left unwired but referenced by a fixture risks the exact approval-bypass D-034 caught (a plain-allow entry with no corresponding `approval_required` row). Reusing that precedent for activation avoids re-litigating it. Uniform approval (no heuristic widening detection) is a deliberate, conservative PRD-posture deviation: a heuristic that decides some proposals are "safe enough" to auto-activate is itself an authority decision, and this slice's job is to prove the propose → approve → activate mechanism works end to end, not to also design a risk-scoring model in the same change. Templates are excluded because a template governs the model's *instruction* surface — unlike a route, workflow, pack, or policy, which shape *authority*, a proposed template would let chat-originated content change what future model calls are told to do, which is a strictly different (and larger) injection-escalation surface than this slice is scoped to close.

## Consequences

- `openspine-schemas::artifact`'s `Lifecycle`/`can_transition` machinery gains its first real runtime caller (`proposed → validated → review_required → approved → active`); quarantine/retire transitions remain schema-only, no runtime path yet.
- A future change wiring a per-kind activation id (e.g. `route.activate` with its own, narrower approval policy) is additive, not a breaking rename, because the per-kind ids were never removed from the PRD-conformant candidate set — only left unwired.
- Proposing a prompt template is a `BadRequest` (`artifact.propose kind must be one of route|agent|workflow|pack|policy`), not a silently-ignored no-op — the boundary is visible to whoever tries it.

## Would change if

A future change deliberately splits activation into per-kind approval policies (e.g. a route needs owner approval but a lower-stakes artifact kind does not) — that would be its own decision, replacing this one's uniformity, not a quiet per-request exception. Similarly, a dedicated prompt-template-authoring change would need its own decision revisiting the injection-escalation tradeoff here, not a silent addition to `PROPOSABLE_KINDS`.

---

# D-049 — Capability specs are backfilled for subsystems implemented inside earlier slices

## Decision

`model-gateway`, `audit-artifact-store`, and `shell-containment` were each implemented as part of earlier build-plan slices (Step 4c/4a) but never got a standalone OpenSpec capability spec. `backfill-implemented-capability-specs` adds one for each, derived from the code and decision log as already shipped, plus restores two dev-process requirements (the `tasks.md`-grants-no-runtime-access scenario, and the archive-must-preserve-rationale bullet list) that were silently dropped when `openspine-development-process`'s canonical spec was condensed from its original delta. Going forward, a change implementing a security-load-bearing subsystem MUST add that subsystem's capability spec in the same change — this is now an ADDED requirement on `openspine-development-process`.

## Rationale

A capability without a spec is unreviewable: there is no single place stating what a subsystem is supposed to guarantee, so a future change can silently regress it without any spec-validation catching the drift. Backfilling now, before the artifact-lifecycle slice adds a fourth authority-sensitive subsystem, closes the gap while the shipped behaviour is still fresh and directly inspectable in the code.

## Consequences

- `openspec validate --all --strict` now covers 10 capabilities instead of 7.
- Every requirement in the three new specs cites the enforcing test where one exists, so the specs cannot silently drift from what `cargo test` actually proves.

## Would change if

A subsystem's behaviour changes enough that its backfilled spec no longer matches reality — at that point the spec must be updated in the same change that changes the behaviour, per the new development-process requirement this decision adds.

---

# D-050 — `max_model_calls` is enforced with an atomic upsert, not a count-then-compare

## Decision

`POST /v1/model/generate`'s `max_model_calls` budget check counted `conversation_state` rows for role `"user"` with a plain `SELECT COUNT`, compared to the limit in application code, then appended the new turn afterwards. Two concurrent requests on the same grant could both read the same pre-increment count and both be allowed through, exceeding the budget by the number of racing callers. Found in an independent adversarial review of `harden-approval-and-budgets` and `implement-artifact-lifecycle-slice`, after both had already merged. Fixed by adding a `grant_counters.model_calls` column and a `try_count_model_call` method with the same atomic `INSERT ... ON CONFLICT DO UPDATE ... WHERE model_calls < ?` upsert pattern `try_count_artifact_put` (D-046) already used for the artifact-put budget — the `WHERE` clause makes the check-and-increment a single SQL statement, so two concurrent callers racing for the same last call can never both pass.

## Rationale

D-046 correctly reasoned that `try_count_artifact_put` needed to be atomic, but `max_model_calls` was left as a separate count-then-compare because it reused an existing `conversation_state` table rather than introducing a new counter — the atomicity gap this created was not caught by any test, since the existing coverage (`max_model_calls_of_one_denies_the_second_call_with_a_single_provider_hit`) only issues calls sequentially. A budget that can be exceeded under concurrency is not a budget; kernel-dispatch-side enforcement (D-046's placement decision) is only as good as the primitive backing it.

## Consequences

- `grant_counters` gains a `model_calls` column (ad-hoc `ALTER TABLE`, safe against pre-existing databases per the existing migration pattern); `count_conversation_turns` is removed as dead code, no other caller used it.
- A new regression test (`store::budget_support_tests::try_count_model_call_allows_exactly_one_concurrent_winner_at_max_one`) spawns real OS threads racing the same grant and asserts exactly one wins — sequential tests alone cannot prove a concurrency invariant.
- `store/mod.rs`'s migration logic was split into its own `migrations.rs` module (mirroring the existing `budget_support`/`gate_support` split) to stay under the 500-line file-size gate after the new migration statement was added; the new test was similarly split into `store/budget_support_tests.rs` to keep `store/tests.rs` under the same gate.

## Would change if

A future budget-style check is added against a plain `SELECT COUNT` (or any other check-then-act pattern) without routing it through an atomic upsert — this decision's precedent is that every grant-scoped counter enforced kernel-dispatch-side must be atomic at the SQL layer, not just correct under sequential testing.

---

# D-051 — The agent-OS canon (AD-001..153) is decomposed into a dependency-edged change sequence; the stale later-changes placeholders are superseded or subsumed

## Decision

The settled agent-OS design canon (`.raw/openspine-agentos-design-log.md`, AD-001..153) is decomposed into loop-executable change briefs in `openspec/openspine-change-sequence.md`, under AD-145's contract: the per-brief `Requires:` lines are the only authoritative ordering statement (dependency edges, no total order); implementation order is delegated to the dev loop; edges marked HARD block a change until the prerequisite is archived. Requirement content stays in the design log — the sequence file holds only decomposition, edges, scope boundaries, and completion criteria, so the two cannot drift into competing sources of truth. The previous "later changes" placeholders are retired: `implement-secret-intake` is carried forward as a brief; the rest are superseded or subsumed per the sequence file's "Reconciliation of the previous later-changes list" section, which is the authoritative disposition mapping. The stale `openspec/openspine-change-backlog.md` (whose "near-term sequence" was fully archived) is deleted for the same reason.

## Rationale

AD-145 made "spec everything; order is the loop's concern" canon, which requires a decomposition artifact a fresh-context loop can execute standalone: eligibility must be computable (all `Requires:` archived), prerequisites must be explicit rather than remembered, and design prose must not be promotable past an unmet prerequisite. The old placeholder list predated the agent-OS round and no longer described real work: keeping stale change names alongside the new sequence would hand the loop two conflicting to-do surfaces. Every disposition is a mapping, not a drop — each placeholder's intent is either already archived or named in a specific successor brief, so no requirement is silently narrowed (D-049 spec-debt precedent).

## Consequences

- `openspec/openspine-change-sequence.md` becomes the loop's single entry point: canon-source precedence, the kernel-invariant checklist, the cross-cutting axioms, the per-change ceremony, the leaning/open policy, and the amendment rule are each stated once there, never restated here.
- `implement-skill-artifact-class` carries a REQUIRED first task: a formal D-0XX revisit of D-048 grounded in the gate-containment guarantee before any runtime skill machinery ships.

## Would change if

The design log gains new settled entries that don't map onto an existing brief (extend the sequence, don't widen briefs), or the loop discovers an edge the decomposition missed — in either case the fix is a new D-0XX plus a sequence amendment, never an in-flight scope stretch of a running change.

---

# D-052 — Archive applies deltas mechanically via `openspec archive --yes`; pre-seeded requirements are carried as MODIFIED; the `--skip-specs` hand-apply ceremony is retired

## Decision

The per-change archive ceremony becomes `openspec archive <id> --yes` followed by `openspec validate --all --strict`. Delta requirements that already exist in a pre-seeded `openspec/specs/<capability>/spec.md` MUST be authored as `## MODIFIED Requirements`, never re-`ADDED`. `--yes` is permitted ONLY on `openspec archive` in non-interactive runs; it remains forbidden on every other openspec command. `--skip-specs` is reserved for changes with genuinely no spec impact (tooling/docs); the previous pre-seeded-conflict workaround — `--skip-specs` plus copying deltas into `openspec/specs/` by hand — is retired. This narrows the blanket "`-y` forbidden" convention.

## Rationale

Empirical probes against openspec 1.5.0 and 1.6.0-beta.1 (PR #37) showed there is no flag-free unattended archive path: plain `openspec archive` presents an interactive `Proceed with spec updates? (Y/n)` prompt that dies (exit 1) in a non-TTY with closed stdin and hangs otherwise — in BOTH versions, so waiting for a stable release cannot fix it. `ADDED` deltas against pre-seeded specs hard-fail ("already exists"), while `MODIFIED` deltas strict-validate and are applied mechanically by `archive --yes` with a green post-apply corpus. Hand-copying deltas into the spec corpus was the single most error-prone step an unattended loop performed on canon; mechanical apply plus strict validation replaces it. The archive confirmation prompt is confirmation theater without a human at the TTY — the real human gate is PR review, which is unchanged.

## Consequences

- `openspec/openspine-change-sequence.md` ceremony bullet rewritten accordingly; the loop inherits the rule from there.
- The generated OMP files (`.omp/skills/openspec-archive-change/SKILL.md`, `.omp/commands/opsx-archive.md`) carry the same ceremony; `scripts/check-omp-ceremony.sh` (wired into `scripts/check.sh`, hence CI) fails the gate if regeneration by `openspec init/update --tools oh-my-pi` silently reverts them or reintroduces dangling skill references.
- A change whose archive fails "ADDED failed... already exists" is mis-authored: the fix is correcting the delta header to MODIFIED, not a `--skip-specs` bypass.

## Would change if

Upstream openspec grows a first-class non-interactive archive mode (no confirmation prompt without `--yes`, or a config knob), or drops the pre-seeded `ADDED` conflict — then the `--yes` carve-out narrows or disappears. If a future change legitimately needs to re-seed an entire capability spec, that is a REMOVED+ADDED (or RENAMED) delta question, decided then, not a return to hand-applied deltas.

---

# D-053 — Kernel extension points are compiled-in registries; a curated canonical `ActionCatalog` makes unknown action ids fail fast at composition and gate

## Decision

The four kernel extension points become registries (`refactor-kernel-registries`, kernel-readiness item 1): a `ConnectorRegistry` (typed slots, Gmail's absence observable), an `ActionHandlerRegistry` for allowed-action dispatch (lookup miss ⇒ the honest stub, never a 500; `email.create_draft`/`artifact.activate` deliberately unregistered), a post-approval resolution table (`artifact.activate` the one non-default entry; the documented default routes to draft creation), and an artifact-kind table as the single source of truth for the five proposable kinds. A canonical `ActionCatalog` of known action ids is a curated const in the kernel (`action_catalog.rs`), NOT derived from fixtures: `compose_authority` returns a structured `UnknownActionId { id, source }` outcome (no grant minted, audited as `authority.unknown_action_id`) for any candidate id outside it, and `gate()` denies a catalog-unknown request with `DenialReason::UnknownAction`, distinct from `NotGranted`.

## Rationale

Match-arms scattered across the kernel made every extension a multi-file edit and let fixture typos ride silently into grants (an unknown id in a pack was indistinguishable from a deliberate grant entry until it was dispatched as a stub). Deriving the catalog from fixtures would make a typo self-legitimizing — the curated const is the review surface. At gate, "outside the action universe" and "known but not granted" are different diagnoses: conflating them under `NotGranted` hides configuration defects from the audit trail.

## Consequences

- Adding a connector/action/artifact kind is a registration at one declared point; a fixture referencing a new action id fails `canonical_catalog_covers_all_fixture_action_ids` until the catalog is deliberately extended.
- Known-but-unimplemented ids (`route.activate`, `workflow.invoke:approved`, `memory.read:owner_preferences_limited`, ...) remain composable and stub-dispatched — the catalog gates existence, not implementation.
- The `Connector` trait's `name()`/`iter()` enumeration seam is the registration surface AD-060/AD-103 will build on.

## Would change if

Runtime-registered actions/connectors ever become a requirement (they are deliberately compile-time today; runtime growth stays behind the artifact-lifecycle approval path), or the catalog moves into a signed artifact so deployments can extend the action universe without a rebuild — then the curated-const stance is revisited under the same fail-fast semantics.

---



# D-054 — Pipeline stages are a typed compiled-in sequence the driver executes; lanes are compiled-in data records

## Decision

The pipeline stages are a typed compiled-in sequence the driver executes, and lanes are compiled-in data records (`refactor-pipeline-driver`, kernel-readiness item 2). `PipelineStage` is a typed enum declared once, with its canonical order fixed as `PipelineStage::SEQUENCE` (nine variants: `Event`, `Verify`, `Identify`, `Route`, `Compose`, `Grant`, `Run`, `Gate`, `Audit`) and its synchronous prefix derived from that as `PipelineStage::SYNC_PREFIX` (the sequence truncated before `Gate`); the driver's execution is checked against `SYNC_PREFIX` — `run_pipeline` records an instrumented executed-stage trace that tests pin equal to the prefix for every lane, and `SYNC_PREFIX` is derived element-by-element from `SEQUENCE` so the declarations cannot drift; the enum is the stage plan the driver is held to, not documentation: `event → verify → identify → route → compose → grant → run`. Lanes are compiled-in `LaneSpec` data records with a hard single-stage hook contract: a lane hook takes typed inputs and returns typed outputs for exactly one stage, and MUST NOT call `resolve_route`, `compose_authority`, `insert_task_grant`, or `run_task`, MUST NOT emit audit for any stage other than its own, and MUST NOT invoke another hook or stage; lanes are kernel values with no runtime registration, mutation, or removal path — never runtime-proposable artifacts. Gate is a distributed runtime stage at the effect boundary (AD-120, D-004), outside the driver prefix: the driver type names `Gate` so the sequence is honest, but execution stays at the shell dispatch surface and the approval callback, and the driver module never calls `gate()`. Lanes carry no sequencing capability — they cannot reorder or omit stages; the driver owns the order via `SYNC_PREFIX`, and per-lane "skips" (owner-control has no preflight verification) are expressed as no-op inputs to that stage, so the stage still runs in order. Finally, `event.received` is emitted only after `Verify` succeeds, preserving today's preflight-failure audit surface: no `event.received` is ever emitted on a preflight-failure path.

## Rationale

Canon (AD-120 and the agent-OS round) never fixed the representation of stages or lanes, only their behavior. A runtime-proposable lane artifact would let approved YAML alter verification order — authority-sensitive machinery this behavior-preserving change must not introduce; runtime lane growth, if ever wanted, goes through the artifact-lifecycle approval path as its own change. The nine-stage listing puts gate after run because effects happen when the shell dispatches intents; the kernel gates each intent at the effect boundary (AD-120, D-004), so the driver names `Gate` to keep the sequence honest while execution stays distributed and the driver module never imports or calls `gate()`. The driver owns order via `SYNC_PREFIX` and a `LaneSpec` carries no sequencing capability, so per-lane variation is expressed as no-op inputs rather than stage omission. Both shipped flows emit the audited envelope only after verification succeeds; pinning that placement in the driver means preflight failures never emit `event.received`, which is exactly today's audit surface and must be preserved by a behavior-preserving refactor.

## Consequences

- `PipelineStage::SEQUENCE` pins the nine stages in canonical order and `PipelineStage::SYNC_PREFIX` derives the synchronous driver prefix before `Gate`; the driver's executed-stage trace is held to `SYNC_PREFIX`, so the enum is the stage plan the driver is checked against — tests assert `SEQUENCE` pins order and an instrumented driver run's executed-stage trace equals `SYNC_PREFIX` for both lanes.
- `LaneSpec` values (`owner_control_lane()`, `email_preview_lane()`) are compiled-in kernel constants with no runtime registration, mutation, or removal path; a lane hook that reimplements a stage body or calls `resolve_route`/`compose_authority`/`insert_task_grant`/`run_task`, emits cross-stage audit, or invokes another hook fails the contract and review.
- `gate()` call sites (`api/actions.rs`, `api/generate.rs`, `pipeline/approval.rs`) and the driver module stay independent; the driver module never imports or calls `gate()`, preserving the structural boundary required by this change.
- `event.received` placement is pinned post-`Verify`; tests assert no `event.received` is emitted on any of the four `/draft` preflight-failure paths (`selection.gmail_not_configured`, `route.refused_uncontained`, `selection.thread_not_found`, `selection.gmail_error`).

## Would change if

Runtime-proposable lanes ever become a requirement — a runtime lane artifact would let approved YAML alter verification order, which is authority-sensitive, so runtime lane growth stays behind the artifact-lifecycle approval path as its own change rather than folded into this behavior-preserving refactor. Equally, if gate were moved into the driver prefix (it stays distributed at the effect boundary per AD-120/D-004), or a lane needed genuine stage-level sequencing capability (forbidden by the `SYNC_PREFIX` invariant today), the compiled-in-sequence stance would be revisited under the same "lanes cannot reorder stages" constraint.

# D-055 — Gate trusted paths are hardened: carve-outs are enumerated catalog data; KernelOrigin is approval-exempt, audit-never-exempt; selection-token validation lives in pure gate() with dispatch-side consumption; digests are kernel-re-derived at approval-effect time

## Decision

The gate's trusted-path surface is hardened along four settled axes. (1) Every effectful path that reaches around `gate()` is enumerated as data in the `ActionCatalog` — classified as `gated-shell`, `post-gate-approved-effect`, `kernel-origin-gated`, or `internal-maintenance-non-effect` — and each enumerated entry has a dedicated characterization test asserting its gate-decision and audit-event behavior (D-055.1). (2) A new `ActionOrigin::{Shell, Kernel}` marker distinguishes shell intent from kernel effect; kernel-origin actions in the enumerated trusted-origin set route through `gate()` with the `Kernel` origin — approval-exempt (auto-allowed) but never audit-exempt, emitting `AuditMeta` unconditionally; a kernel-origin call for an action outside the set is denied (D-055.2). (3) For catalog-marked `token_requiring` actions, `gate()` itself validates the selection token — grant-bound, exists, correct type, unexpired — inside its pure, no-I/O decision; the atomic single-use consume stays at dispatch so `gate()` never mutates state (D-055.3). (4) Shell-facing request DTOs carry no digest fields; the kernel re-derives the payload digest from artifact-store bytes at approval-effect time and denies the effect on any mismatch with the approved digest, never trusting a shell-supplied digest string (D-055.4). The validate-in-gate / consume-at-dispatch split preserves `gate()`'s purity and follows the dispatch-side enforcement precedent of D-046/D-050.

## Rationale

The original gate trusted-paths were implicit and scattered: `notify_owner_best_effort` bypassed approval with an ad-hoc carve-out (D-046), and selection-token validation lived inside the dispatch body (`api/actions.rs:384-421`) rather than in the pure decision, so a `gate()` test could not assert token behavior and a refactor could silently drop the check. Enumerating the carve-outs as catalog data makes the trust surface a reviewable, finite set and forces a characterization test per entry, turning "we hope nothing reaches around gate()" into "these eight enumerated paths are the only ones, each proven." Routing every kernel-origin effect through `gate()` with a `KernelOrigin` marker keeps the audit chain total — the kernel is trusted to need no owner approval, but its effects are never invisible — while pure-`gate()` token validation plus dispatch-side consume keeps `gate()` a deterministic, side-effect-free function the gate tests can fully exercise. AD-120's "shell sends intents, kernel computes outcomes" is the same boundary the digest re-derivation enforces: the shell proposes, the kernel derives and verifies; D-041's digest composition is what gets re-derived, and D-050's atomic-upsert placement is the precedent for keeping state mutation (the token consume) at dispatch rather than inside `gate()`.

## Consequences

- `ActionCatalog` gains an enumerated kernel-origin action set (e.g. `owner.notify`) and a per-action `token_requiring` flag; the eight effect paths are classified catalog entries, each with a dedicated characterization test (`notify_owner_best_effort`, `create_approved_draft`, `activate_approved_artifact`, `dispatch_read_selected_thread`, `dispatch_lyra_preview`/`propose_draft_creation`, `dispatch_artifact_propose`, `sweep_expired_grants`, `answer_callback_query`).
- `gate()` takes an `ActionOrigin` (or resolves it from the request/catalog) threaded into `AuditMeta`; kernel-origin calls in the trusted set auto-allow without an approval record but always emit audit; kernel-origin calls outside the set are denied.
- `GateContext::find_selection_token` is called inside the pure `gate()` decision for `token_requiring` actions; the atomic consume remains at dispatch (`api/actions.rs:413-416`), so `gate()` performs no I/O or mutation.
- Shell DTOs (`target_digest`/`payload_ref.digest`) are structurally excluded; `create_approved_draft` (`pipeline/approval.rs:206-359`) re-derives the payload digest from store bytes and denies on mismatch (target re-derivation already at `approval.rs:290`), closing the shell-supplied-digest trust gap (D-041 digests, re-derived per AD-120).
- The single `owner.notified` carve-out of D-046 is generalized into the data-described `KernelOrigin` set; grant-budget enforcement (D-046/D-050 atomic upsert) stays at dispatch, now joined by the token consume as the only dispatch-side state mutation for token-requiring paths.

## Would change if

Runtime-proposable trusted-origin carve-outs ever become a requirement — the enumerated set is deliberately compile-time catalog data today, and making it runtime-editable would let approved YAML alter which kernel effects bypass approval, which is authority-sensitive; such growth stays behind the artifact-lifecycle approval path as its own change. Equally, if `gate()`'s purity constraint were relaxed to allow state mutation, the token consume could move into `gate()` — but that would break the pure-decision tests and the TOCTOU-avoiding dispatch placement established by D-050, so the validate-in-gate / consume-at-dispatch split stands unless that constraint is explicitly revisited.

---

# D-056 — Eval-store groundwork defers AD-111 evaluator policy: only the verdict-landing surface is settled

## Decision

`define-lineage-and-eval-store` lands the non-retrofittable schema groundwork only: a generation/lineage model on proposed-artifact rows (`ArtifactLineage`, root/derived consistency enforced fail-closed at both the write boundary and load) and an indexed `eval_verdicts` table (`recorded_at` persisted as checked epoch-nanosecond INTEGER so chronological ordering is exact across whole-second/fractional boundaries). AD-111 is *leaning* and is cited by this change only for the fact that verdicts land in a dedicated indexed store rather than audit-chain rows. The groundwork does NOT settle judge-independence requirements, evaluator identity semantics, attack-trace evidence semantics, or a verdict vocabulary: `verdict` stays an open string, and `fitness`/`evidence`/`evaluator` are optional forward-compatible metadata (`evaluator` is `Option<String>`).

## Rationale

The change brief cites AD-111 solely for verdict landing; promoting its other *leaning* details to normative spec requirements would canonize an unratified decision without owner review (spec-debt rule, D-049 precedent). Landing the indexed surface now keeps the non-retrofittable schema in place while leaving every policy question open for the later evaluation/prover-judge change.

## Consequences

The eval store is usable by later changes (`implement-overlay-eval-gate`, `implement-model-swap-ceremony`) as a landing surface, but any evaluator policy those changes need must be proposed and ratified there. D-006 keeps verdict rows authority-free; D-011 digest binding is retained via the required `artifact_digest` column.

## Would change if

AD-111 is settled by the owner — the deferred semantics (judge independence, evaluator identity, attack traces, verdict vocabulary) would then land as their own change with a spec delta over this table, potentially tightening `verdict` to an enum via migration.

---
---



---



# D-057 — Counterparty-facing actions are an explicit kernel catalog set

## Decision

A denial receives AD-151's canonical deferral and AD-133 escalation only when the kernel-owned `ActionCatalog` marks the requested action as counterparty-facing. The v1 set contains the existing `email.send` only; owner-channel, internal, draft-only, unknown, and unclassified actions keep ordinary typed enum outcomes. Adding a future external channel requires an explicit catalog entry and classification in the same reviewed change.

## Rationale

The action API has no shell-spoofable counterparty marker, and blanket escalation would expose owner delivery and deferral semantics on internal/owner-only denials.

## Consequences

Escalation surface area grows only through reviewed catalog changes; the deferral/no-leak machinery is exercised on exactly the actions that face a counterparty.

## Would change if

A channel-level counterparty marker becomes kernel-derivable (e.g. from route/persona binding), at which point classification could move from a static catalog set to routed data — via its own reviewed change.

---

# D-058 — Security escalations require result-returning owner delivery

## Decision

`route_escalation` resolves the task's persisted bound owner chat and calls a mandatory gated `owner.notify` path (`notify_owner_required`). Missing-key, gate, and connector failures are recorded as `owner.notify_failed` when that audit append succeeds and are returned as structured errors; audit persistence failures propagate. `action.escalated` is appended only after connector success. Courtesy pipeline notifications may remain best-effort.

## Rationale

A mandatory escalation that silently swallows delivery failure reports success for an owner surface that never happened — the AD-137 untruthful-record class. Full AD-138 dead-letter retry/metrics behavior belongs to `implement-failure-surfacing-contract`.

## Consequences

The API returns a structured failure when escalation delivery fails; the audit trail never claims an escalation the owner did not receive.

## Would change if

The AD-138 dead-letter substrate subsumes this path's retry semantics — the truthfulness contract stays, the retry mechanics may move.

---

# D-059 — Dormant thread bindings are MAC-authenticated before activation

## Decision

`TaskGrant.thread_id` is included in `RootAuthority`'s canonical commitment when populated, while no channel populates or consumes it; when `None`, the key is omitted from canonical bytes so pre-change grants keep verifying. A `None`-to-`Some` rewrite fails MAC verification.

## Rationale

This prevents shell-side rewrites from changing a future conversation binding before the thread-capable channel activation change lands. Activation changes usage and channel integration, not the integrity boundary.

## Consequences

Thread binding ships dormant but tamper-evident; the later activation change needs no MAC-format migration.

## Would change if

The MAC/root payload gains an explicit version field — conditional key omission could then be replaced by versioned canonical shapes.

---

# D-060 — The overlay eval gate's first-cut evaluator is deterministic; the full replay/judge protocol is owner-reserved

## Decision

`implement-overlay-eval-gate` enforces AD-142's structural guarantee — an authority-bearing proposal cannot reach the approval surface without attached replay + judge evidence — using a deterministic first-cut evaluator: an owner-control-history availability gate plus structural artifact probes. Verdicts land in the D-056 eval store using only its settled open schema (open verdict string, optional fitness/evidence/evaluator metadata). Evaluator independence, evaluator identity, attack-trace semantics, and verdict vocabulary remain owner-reserved per D-056; a later owner-ratified evaluator change replaces/extends these probes with the full OQ-17 holdout replay and AD-111 prover-verifier protocol.

## Rationale

The structural cannot-bypass guarantee is the load-bearing property and is achievable deterministically now; settling judge policy here would canonize *leaning* AD-111 semantics without owner review.

## Consequences

Standing rules and later authority-bearing proposals get evidence-gated promotion immediately; evaluator sophistication can grow without touching the promotion boundary.

## Would change if

The owner ratifies the AD-111 evaluator protocol — the probes are replaced under the same promotion boundary and eval-store schema.

# D-061 — Model-swap golden sets use a bounded deterministic first cut

## Decision

The first AD-152 model-swap evaluator uses operator-owned, role-bound golden-set fixtures with at least three standard cases and one adversarial case, deterministic substring criteria, and a maximum of 20 cases. Prompts, criteria, observed excerpts, and owner summaries are bounded. The whole replay is capped at the lesser of five minutes and the grant's remaining wall-clock expiry. Provider calls consume atomically reserved model-call budget even when replay fails or times out.

## Rationale

AD-152 requires evidence-bearing swaps but leaves the first executable golden-set format open. A deterministic bounded format makes the ceremony enforceable now without prematurely settling AD-111's deferred evaluator-independence policy.

## Consequences

Base, matcher, and miner assignments share one governed proposal format. Failed attempts are not a free retry path. Matcher and miner consumers may arrive later without changing the ceremony.

## Would change if

The owner ratifies a richer evaluator protocol under D-056/D-060; it replaces the deterministic criteria behind the same evidence-gated promotion boundary.

---

# D-062 — Active model swaps require symmetric DB and overlay provenance

## Decision

Startup restores a model swap only when the exact normalized active manifest matches the latest persisted Active proposal for that role and version, with passing replay and judge verdicts bound to the proposal digest. A missing, inactive, shadowing, or mismatched overlay fails closed; the kernel never silently falls back to an older or bootstrap provider while a newer Active row exists.

## Rationale

Checking only file-carried digests lets an operator-tree edit bypass the ceremony; checking only rows lets missing files silently roll authority back. The trust boundary requires agreement in both directions.

## Consequences

Manual active swap files without ceremony provenance are rejected. Startup detects deletion, downgrade, shadowing, and tampering instead of changing the live provider silently.

## Would change if

The overlay store becomes a transactional projection derived entirely from the proposal database, eliminating the two-surface reconciliation boundary.

---

# D-063 — Model-swap activation uses a serialized staged recovery protocol

## Decision

Model-swap activation writes a loader-invisible `.pending` candidate, transactionally commits monotonic supersession, `Approved → Active`, and `artifact.activated`, then atomically renames and publishes the registry/provider map under one activation serialization boundary. Startup completes a pending rename only when its canonical bytes match the committed Active proposal digest; uncommitted candidates are removed and tampered candidates are quarantined. Generic artifact kinds retain their existing atomic temporary-write path.

## Rationale

No ordering of filesystem, SQLite, and memory publication is natively atomic. An explicit staged protocol makes every crash window recoverable without exposing unaudited authority or preventing restart.

## Consequences

Concurrent stale callbacks cannot overwrite a newer provider. Transaction failure leaves the prior disk, registry, and provider authoritative. Crash recovery is deterministic and digest-bound.

## Would change if

Artifacts and lifecycle state move into one transactional store with an atomic materialized-filesystem projection.

---


# D-064 — Connector secrets migrate once into the kernel vault

## Decision

Connector credentials move from runtime environment lookup to the encrypted kernel vault. Legacy connector environment variables MAY seed an absent vault slot exactly once at first startup, but MUST NOT be consulted after a slot exists. Connector calls resolve the vault at call time. `OPENSPINE_ARTIFACT_KEY` remains an environment bootstrap because it is the root key required to open the vault.

## Rationale

One-way bootstrap preserves existing installations while making rotation effective without restart and preventing stale environment values from overriding owner-managed credentials.

## Consequences

Telegram and Gmail credentials become vault-authoritative after first seed. Missing or undecryptable slots fail closed.

## Would change if

The root encryption key gains a hardware-backed or external key-management bootstrap that removes its environment dependency.

---

# D-065 — Provider API-key vault migration belongs to foundation amendment

## Decision

Provider API keys remain environment-sourced until a future foundation-amendment change explicitly migrates them into the kernel vault. The archived model-gateway change is not an executable owner for this work.

## Rationale

Provider credentials sit on the kernel model-gateway trust boundary. Migrating them implicitly inside connector secret intake would widen the change and bypass the kernel-amendment ceremony.

## Consequences

This change migrates connector credentials only. Provider-key migration stays explicit, reviewable, and dependency-aware.

## Would change if

The foundation-amendment lane ratifies and implements a provider-key vault migration.

---

# D-066 — Paired Gmail credentials stage until atomic validated promotion

## Decision

A first Gmail credential is stored only in a staging slot and is not validated or active until the paired credential arrives and the connector validates the pair. Promotion is atomic with full-snapshot rollback on any post-mutation failure.

## Rationale

Publishing half of an OAuth credential pair creates a broken live configuration; non-atomic promotion can leave live and staged slots inconsistent after storage or audit failure.

## Consequences

Incomplete pairs never become connector-visible. Failed promotion restores live values, staged values, and staging metadata exactly so the owner can retry.

## Would change if

The provider supports independently valid single-field credentials with no pairwise validation requirement.

---

# D-067 — Telegram poll offsets are namespaced by bot identity

## Decision

The consumed Telegram `update_id` is persisted under the current bot id. A legacy non-namespaced offset migrates into that namespace exactly once and is then cleared. Same-bot token rotation preserves the consumed offset; a different bot starts with a fresh namespace and never inherits the previous bot's offset.

## Rationale

A global offset either suppresses valid updates after bot rotation or replays already-consumed updates. Bot identity, not token text, is the stable delivery cursor boundary.

## Consequences

Same-bot rotation avoids redelivery. Different-bot rotation does not strand the new bot behind another bot's cursor. Migration and bot-id persistence occur transactionally.

## Would change if

Telegram supplies a stronger server-side consumer identity and cursor primitive that survives token rotation without local namespacing.

---

# D-068 — Authenticated API bad requests are not duplicated through owner notification

## Decision

API bad-request failures are surfaced directly to the authenticated caller and are not duplicated through `owner.notify`; connector and resource failures remain digest-batched.

## Rationale

The authenticated API response is already the immediate owner-visible failure surface. Sending the same failure again creates noise without improving durability.

## Consequences

Bad requests remain typed and synchronous. Failures outside that direct response boundary still enter the immediate or digest failure lanes.

## Would change if

An API client cannot reliably surface authenticated error responses to the owner.

---

# D-069 — Kernel connector counters are the minimal observability surface

## Decision

Kernel-persisted connector success/failure counters remain the minimal observability surface until an approved metrics contract exists.

## Rationale

AD-138 requires truthful operational visibility but does not require an external metrics stack. Durable local counters satisfy the current self-hosted boundary without adding infrastructure.

## Consequences

Connector operations update SQLite counters. Counter-persistence failures are surfaced as resource failures and never erase the primary notification audit or retry record.

## Would change if

The day-two operations change ratifies an external metrics/export contract.

---

# D-070 — Retryable owner notifications use encrypted artifact references

## Decision

Retryable owner-notification records MUST reference encrypted artifacts. If artifact persistence fails, the kernel records a plaintext-free audit and digest record rather than creating a blank-body retry.

## Rationale

Persisting notification plaintext in SQLite violates D-012; inserting an empty retry body falsely promises recoverability.

## Consequences

Dead-letter retries are decryptable only through the artifact store. Artifact-store failure remains visible without leaking detail or creating an undeliverable retry.

## Would change if

The retry store itself becomes an encrypted payload store with equivalent reference and erasure guarantees.

---

# D-071 — External owner delivery may be delivery-unknown after a crash

## Decision

External owner delivery is delivery-unknown after a crash between provider send and durable receipt commit; recovery MAY resend. The runtime does not claim exactly-once delivery without connector idempotency support.

## Rationale

SQLite and Telegram cannot share one atomic transaction. Preferring retryability over silent loss necessarily permits duplicates in this crash window.

## Consequences

Receipt completion is transactional and claim-token conditioned. A committed receipt prevents retry; an uncommitted receipt remains eligible and truthfully delivery-unknown.

## Would change if

The connector provides an idempotency key with durable exactly-once semantics.

---

# D-072 — Digest detail retrieval is a secure lossless pagination substrate

## Decision

`/digest <ULID> [page]` provides deterministic, lossless UTF-8 pagination over encrypted detail references with stable item identity and page N/M. AD-082 personality, fold wording, and presentation style remain deferred to personality-seed work.

## Rationale

The kernel must make every retained failure byte retrievable without owning the future assistant presentation layer or exceeding Telegram's message bound.

## Consequences

Only the authenticated owner can retrieve detail. Successful page delivery records detail-specific receipts. Unavailable pages remain unresolved and truthfully audited; failed deliveries remain retryable until proven delivery.

## Would change if

A ratified presentation layer supplies an equivalent bounded retrieval contract without weakening losslessness or owner authentication.

---

# D-073 — Durable workflow steps persist intent before effect and replay recorded outcomes

## Decision

A workflow run records every outside-world step — time, randomness, model and connector calls, approvals, timers — as a ledger-backed intent before its effect executes, keyed by an exact stable step identity. Crash recovery rehydrates recorded outcomes and never re-runs a recorded effect. A `Pending` non-idempotent step with no durable receipt fails closed on recovery: the runtime refuses to re-dispatch absent provider idempotency, leaving retry an explicit caller obligation. Step payloads that persist inline are a sealed, closed, non-secret set.

## Rationale

SQLite and external connectors cannot share one atomic transaction, so exactly-once effects are unattainable; recording intent first and refusing receiptless re-dispatch prefers truthful loss-surfacing over silent duplication, and the sealed payload set keeps D-012 plaintext discipline structural.

## Consequences

Replay after a crash is deterministic against the persisted ledger under one read snapshot. A crash between dispatch and receipt leaves the step `Pending` and surfaced rather than silently re-run. Callers needing automatic retry must supply an idempotent effect path.

## Would change if

Connectors gain durable idempotency keys, allowing recovery to re-dispatch receiptless pending effects safely.

---

# D-074 — Workflow timers fire at most once via trusted-clock atomic claims

## Decision

Workflow timers are kernel-owned rows fired by the kernel timer driver at most once: firing performs an atomic claim (compare-and-set on the pending row keyed by exact timer identity) using the trusted kernel clock carried into both due-selection and the claim predicate; consumers only schedule and observe `workflow.timer_fired` ledger events.

## Rationale

AD-104's dark-window requirement needs a driver that cannot double-fire across crash/restart races, and caller-supplied timestamps must not be able to fire timers early.

## Consequences

A timer fires exactly once per claim even under concurrent drivers; a crash after claim but before handler effect surfaces through the ordinary step contract rather than a second fire. Timer effects are classified `InternalMaintenanceNonEffect` in the D-055 catalog and never pass the gate.

## Would change if

Timer handlers acquire effects requiring gated authority, which would move firing onto the granted action path.

---

# D-075 — The spend kill switch accounts globally but pauses only non-immediate lanes

## Decision

The AD-143 daily spend kill switch counts every model invocation (including bounded model-swap golden-set evaluation) and every connector call — grant-bound effects, control-plane polling, callback acknowledgements, credential validation probes — in one durable UTC-day ledger with atomic reserve-and-check. On breach, only non-immediate (proactive/headless) lanes are paused, at both grant composition and action dispatch. Owner-control immediate effects remain live and counted, cap-exempt by lane; control-plane operations remain live and counted; a dedicated notification-only reservation keeps the breach notification deliverable. Breach marking is transactional with the denial decision, and a durable alert state is consumed only on confirmed delivery or a confirmed durable dead letter.

## Rationale

AD-143's "across all model calls and connector usage" is accounting scope; its enforcement contract pauses the proactive and headless lanes and requires owner notification on the immediate lane. Denying the owner's own control lane would contradict that contract, while exempting anything from accounting would falsify the ledger.

## Consequences

After breach the owner keeps a live, fully accounted control channel while autonomous spending stops. Reception (polling/acks) cannot self-deny the daemon. Every exemption is visible in the ledger rather than invisible to it.

## Would change if

Lanes gain per-lane budgets, or a ratified decision classifies evaluation calls as non-production spend.

---

# D-076 — Spend caps are required finite configuration

## Decision

`spend_cap.model_calls_per_day` and `spend_cap.connector_calls_per_day` are required finite values in the deny-unknown-fields configuration schema. There is no absent/disabled state: an operator who wants an effectively unlimited cap must set an explicitly large number.

## Rationale

A silently missing cap is indistinguishable from a misconfigured one; requiring an explicit value makes disabling the kill switch a visible, reviewable act.

## Consequences

Config parsing fails loudly without caps. Example configurations carry finite values.

## Would change if

A ratified budget hierarchy replaces the flat daily caps.

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

1. ~~Gmail OAuth scopes for read selected thread, create draft, and whether send authority is bundled.~~ Closed by D-029: `gmail.readonly` + `gmail.compose` requested together; `gmail.send` never requested (hard-denied at the gate regardless).
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
| 2026-07-02 | Added D-036 (Phase-2 thread selection via a kernel-recognized `/draft <thread_id>` command) and D-037 (Gmail OAuth token exchange via a plain refresh-token POST, `base64` promoted to a direct dependency, no `oauth2` crate), discovered while implementing Step 5 (`implement-selected-thread-email-preview-slice`). |
| 2026-07-02 | Added D-038 (retroactively documenting `resolve_owner_identity`'s already-implemented caller-supplied `channel_trust`, cited by code comments but never recorded) and D-039–D-044 (Telegram inline-button approval channel, pending-`ActionRequest` persistence, `email.create_draft` digest composition, kernel-derived reply recipient, `lyra.ui.preview` extended to propose+persist+button, kernel-side approved-draft dispatch), discovered while implementing Step 6 (`implement-digest-bound-draft-approval`). |
| 2026-07-03 | Added D-045 (WYSIWYS: truncated previews refuse approval buttons), D-046 (grant budgets enforced kernel-dispatch-side; artifact budget counts shell-initiated puts only), and D-047 (task tokens hashed at rest, redacted from persisted grant JSON, 24h expired-grant sweep), discovered while implementing `harden-approval-and-budgets`. |
| 2026-07-03 | Added D-048 (`artifact.activate` is the single canonical activation action id, mirroring D-034's precedent; uniform owner approval for every proposable kind; prompt templates excluded from proposable kinds), discovered while implementing `implement-artifact-lifecycle-slice`. |
| 2026-07-03 | Added D-049 (capability specs backfilled for model-gateway, audit-artifact-store, and shell-containment; future security-load-bearing subsystems must gain their spec in the implementing change), discovered while implementing `backfill-implemented-capability-specs`. |
| 2026-07-03 | Added D-050 (`max_model_calls` enforced with an atomic upsert instead of a count-then-compare, closing a concurrent-request TOCTOU gap; `count_conversation_turns` removed as dead code), found in an independent post-merge review of `harden-approval-and-budgets` and `implement-artifact-lifecycle-slice`. |
| 2026-07-07 | Added D-051 (agent-OS canon AD-001..153 decomposed into the dependency-edged change sequence in `openspec/openspine-change-sequence.md` per AD-145; stale later-changes placeholders superseded/subsumed with explicit mappings; `implement-secret-intake` carried forward), the spec-round decomposition artifact for the unattended dev loop. |
| 2026-07-09 | Added D-052 (archive ceremony: `openspec archive --yes` applies deltas mechanically, pre-seeded requirements carried as MODIFIED, `--skip-specs` hand-apply retired, `--yes` permitted only on non-interactive archive; guarded by `scripts/check-omp-ceremony.sh`), settled after empirical archive probes of openspec 1.5.0 / 1.6.0-beta.1 on PR #37. |
| 2026-07-10 | Added D-053 (kernel extension points become compiled-in registries — connector, action-handler, post-approval, artifact-kind; curated canonical `ActionCatalog` makes unknown action ids a hard `UnknownActionId` composition error and a structured `UnknownAction` gate denial distinct from `NotGranted`), settled while implementing `refactor-kernel-registries`. |
| 2026-07-10 | Added D-054 (pipeline stages are a typed compiled-in `PipelineStage` sequence the driver executes from its synchronous `SYNC_PREFIX` — `event → verify → identify → route → compose → grant → run`; lanes are compiled-in `LaneSpec` data records with a single-stage hook contract, never runtime-proposable artifacts; gate is a distributed runtime stage at the effect boundary per AD-120/D-004, outside the driver prefix — the driver module never calls `gate()`; lanes cannot reorder or omit stages; `event.received` is emitted only after Verify succeeds, preserving the preflight-failure audit surface), settled while implementing `refactor-pipeline-driver`. |
| 2026-07-10 | Added D-055 (gate trusted paths hardened along four axes: (1) every effectful path reaching around `gate()` is enumerated as classified `ActionCatalog` data — `gated-shell` / `post-gate-approved-effect` / `kernel-origin-gated` / `internal-maintenance-non-effect` — with a dedicated characterization test per entry; (2) `ActionOrigin::{Shell, Kernel}` marks kernel-origin effects that route through `gate()` approval-exempt but audit-never-exempt, generalizing D-046's single `owner.notified` carve-out into a finite trusted-origin set (outside the set ⇒ denied); (3) selection-token validation moves into the pure, no-I/O `gate()` decision via `GateContext::find_selection_token` while the atomic single-use consume stays at dispatch, preserving `gate()`'s purity; (4) shell DTOs carry no digest fields and the kernel re-derives payload/target digests from artifact-store bytes at approval-effect time, denying on mismatch and never trusting a shell-supplied digest (per D-041 digests and AD-120's shell-intents/kernel-outcomes boundary); the validate-in-gate / consume-at-dispatch split follows the D-046/D-050 dispatch-side enforcement precedent), settled while implementing `harden-gate-trusted-paths`. |
| 2026-07-16 | Added D-056 (eval-store groundwork defers AD-111 evaluator policy: only the indexed verdict-landing surface is settled — open verdict string, optional fitness/evidence/evaluator metadata, checked epoch-nanosecond timestamps, fail-closed lineage consistency; judge-independence, evaluator identity, attack-trace evidence semantics, and verdict vocabulary return to the owner with the later evaluation change), settled during review of `define-lineage-and-eval-store`. |
| 2026-07-16 | Added D-057 (counterparty-facing actions are an explicit kernel ActionCatalog set, v1 = `email.send` only; only such denials get the canonical deferral + escalation), D-058 (security escalations require result-returning gated owner delivery; `action.escalated` only after connector success; failures recorded as `owner.notify_failed` and returned as structured errors), and D-059 (dormant `thread_id` bindings are MAC-authenticated when populated, omitted from canonical bytes when `None` for legacy compatibility), settled while implementing `implement-escalation-and-refusal`. |
| 2026-07-16 | Added D-060 (the AD-142 overlay eval gate ships a deterministic first-cut evaluator — owner-history availability gate + structural probes — with verdicts in the D-056 eval store; the full OQ-17 holdout replay and AD-111 prover-verifier protocol remain owner-reserved), settled while implementing `implement-overlay-eval-gate`. |
| 2026-07-16 | Added D-061 (bounded deterministic first-cut model-swap golden sets with grant-bounded timeout and consumed attempt budget), D-062 (symmetric Active proposal ↔ exact overlay provenance required at startup), and D-063 (serialized staged model-swap activation with transactional lifecycle/audit and digest-bound crash recovery), settled while implementing `implement-model-swap-ceremony`. |
| 2026-07-16 | Added D-064 (one-way connector-secret migration into the encrypted kernel vault with call-time resolution), D-065 (provider API-key migration owned by the foundation-amendment lane), D-066 (paired Gmail credentials stage until atomic validated promotion), and D-067 (Telegram offsets namespaced by bot identity with one-time legacy migration), settled while implementing `implement-secret-intake`. |
| 2026-07-17 | Added D-068 (direct authenticated API bad-request surfacing without duplicate owner notification), D-069 (kernel connector counters as the minimal observability surface), D-070 (encrypted artifact references for retryable owner notifications), D-071 (delivery-unknown send-to-receipt crash semantics), and D-072 (secure lossless `/digest` pagination substrate with presentation deferred), settled while implementing `implement-failure-surfacing-contract`. |
| 2026-07-17 | Added D-073 (durable workflow steps persist intent before effect, replay rehydrates recorded outcomes, receiptless pending non-idempotent steps fail closed with sealed inline payload set) and D-074 (kernel-owned workflow timers fire at most once via trusted-clock atomic claims), settled while implementing `implement-durable-workflow-replay`. |
| 2026-07-17 | Added D-075 (the daily spend kill switch accounts for every model and connector call while breach pauses only non-immediate lanes; owner-control and control-plane operations stay live, counted, cap-exempt; notification-only reservation keeps breach alerts deliverable) and D-076 (spend caps are required finite configuration), settled while implementing `implement-spend-kill-switch`. |

