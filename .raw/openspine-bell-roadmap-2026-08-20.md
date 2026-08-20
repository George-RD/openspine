# OpenSpine Roadmap: Prioritized Tickets

Source documents, committed in this repo alongside this roadmap: the external architecture review and the Effect Truth & Action Mediation Convergence Spec. This roadmap turns their findings, the known open issues, and the requirements of a first external consumer (below) into an ordered ticket queue. Orchestration-agnostic: run tickets in any loop; priorities and dependencies govern order.

## The first external consumer: Bell

OpenSpine is being repositioned as a runtime that products consume: Lyra becomes the reference assistant package, and a first commercial consumer, called Bell, is in specification in a separate repository. Bell is a multi-tenant AI receptionist for small businesses, reached by their customers over WhatsApp and a web chat widget. It matters to this repo only through the concrete requirements it imposes:

- Multi-tenancy: many isolated businesses (tenants) on one deployment. Identity, grants, and audit need (tenant, principal, contact) as native dimensions. Today the runtime assumes a single owner.
- Adversarial principals as the common case: most inbound events come from unauthenticated end customers, who must be treated as attackers. Today most inbound events come from the trusted owner.
- Per-tenant owners on WhatsApp: every tenant has an owner needing approval/review surfaces, on WhatsApp, not one Telegram chat.
- A WhatsApp Cloud API connector as a second protocol: webhook ingress with Meta signature verification, kernel token custody, pre-approved template egress, provider message-id (wamid) recording. Materially different from Gmail (no mailbox/thread semantics; a 24-hour service window; interactive button replies).
- Code-gated proactive outbound: follow-up and reminder messages gated deterministically on opt-out status, quiet hours, per-contact caps, and budget, never by model judgment.
- A possibly non-Rust worker: Bell's conversation logic wants to iterate in TypeScript, speaking the kernel HTTP contract as a contained worker.
- Bell's product data (contacts, message threads, scheduled messages) is planned for its own Postgres with row-level security, separate from the kernel's SQLite.

A set of security invariants distilled from Bell's trust model recurs in tickets below by number:
- I1 Tenant closure: every read/write scoped to one tenant, bound from verified channel identity, never from model output.
- I2 Principal-scoped capabilities: a session's toolset fixed by verified role before inference; privileged tools structurally absent from untrusted sessions, not refused.
- I3 Contact closure: an end-customer session can address exactly one contact (the verified sender), pre-resolved by the kernel.
- I4 Visibility-classed context: data carries a visibility class (customer-visible / staff / owner); context assembly includes only classes at or below the session principal.
- I7 Gated outbound: proactive sends pass deterministic gates (opt-out, time window, caps, budget) in code.
- I8 Human hold: a thread taken over by a human is closed to the agent until explicit hand-back or timeout.
- I9 Confirm-before-mutate: owner commands that change state are echoed and confirmed in a fresh message before applying.

Two integration milestones referenced below:
- Integration spike: a thin vertical slice of Bell built against this runtime (inbound event -> tenant-scoped grant -> one scope-bound tool -> template send -> audit), planned to start once P0 is done.
- Fit review: a human decision, after the P1 assessment tickets and the spike, on whether Bell builds on this runtime or beside it. P3 tickets wait for that review.

## Conventions
- Type BUILD changes code: TDD mandatory, check.sh green per commit, fail-closed behaviour never weakened. Untouchable: the five-crate trust architecture, pure gate and authority modules, kernel-only credential holding, independent catalog/declaration/executor axes.
- Type ASSESS produces reports and tests only. Never modify production code to make an assessment pass; a failing test recorded honestly is the deliverable.
- Every ticket closes with a summary comment: done, found, skipped, decisions needed.
- Issues filed from the Bell repo get the label bell-feedback and reference the invariant (I1-I9) or spec section they serve. Bell never patches around the kernel; the kernel changes or the finding escalates.

---

## T0 [ASSESS] Reconcile this roadmap against the live issue tracker
Drafted against #117, #118, #128, #129, #131, #132, #173-#177 as described in the review documents; the live tracker may hold more. Map every open issue to a ticket (extends / duplicates / unrelated-triage), flag contradictions, propose additions. Blocks nothing; do first anyway.

## P0: Correctness

### T1 [BUILD] Effect Truth Slice A: characterise and reproduce (relates #173, #174)
Map the current action -> connector -> reservation -> audit flow; document every conversion that can lose attempted/not-attempted/unknown information; add regression tests reproducing #173 and #174. STOP CONDITION: if the defects cannot be reproduced, stop the Effect Truth track, write up observations, re-plan T2/T3.
Depends: nothing. Blocks: T2.

### T2 [BUILD] Effect Truth Slice B: typed EffectDisposition
Introduce EffectDisposition (NotAttempted / ConfirmedSuccess / ConfirmedFailure / DeliveryUnknown) with classification at the provider boundary; Gmail classifies 429/5xx/timeout-after-transmission as DeliveryUnknown unless the response proves non-occurrence. Behaviour unchanged except the proven defects.
Depends: T1. Blocks: T3.

### T3 [BUILD] Effect Truth Slice C: disposition-driven settlement (closes #173, #174)
Settlement determined solely by disposition (NotAttempted/ConfirmedFailure cancel; ConfirmedSuccess finalizes; DeliveryUnknown retains and fences); remove caller-side interpretation of generic errors; production-entering tests for all four dispositions (spec section 15); adversarial pass (section 16) then simplification/deletion pass (section 17). Note: the future WhatsApp connector (T19) has the identical ambiguity profile, and its failure mode is a duplicate message to an end customer; this ticket is its foundation.
Depends: T2. Blocks: T17, T19.

### T4 [BUILD] #175: leaked activation temp files
As filed. Depends: nothing.

### T5 [BUILD] #176: boot-blocking whole-table parse blast radius
As filed. Depends: nothing.

### T6 [BUILD] #177: one internal fault-injection seam
Replace scattered one-shot test fault flags with a single internal seam; add the missing production-entering tests. No public API. Depends: nothing; pairs well after T3.

## P1: Bell fit assessment (feeds the fit review)

### T7 [ASSESS] Invariant test suite
tests/bell-invariants/ encoding I1, I2, I3, I4, I7, I8, I9 (definitions above) as tests against the kernel as built, marked expected-fail where the concept doesn't exist yet. Where tenancy must be stubbed to express a test, stub in test code only and document the stub as a finding.
Depends: nothing (runs against current main; re-run after T3).

### T8 [ASSESS] Tenancy expressibility design report
Can openspine-schemas and openspine-authority carry (tenant, principal, contact) as native dimensions of identity, grants and audit without violence to the design? Design document with proposed schema shape. No implementation.
Depends: nothing. Blocks: T21.

### T9 [ASSESS] Two-store ruling report
The kernel is a single-owner SQLite runtime; Bell plans its product data in Postgres with RLS. Analyse the split (kernel SQLite for grants/reservations/audit; product Postgres for tenant data), name the consistency seam, enumerate failure modes at the seam, recommend.
Depends: nothing.

### T10 [ASSESS] Owner-surface report (relates #129)
Can OwnerSurfaceRef be per-tenant and channel-plural (many owners, on WhatsApp, not one Telegram chat)? Inventory every bound_chat_id / Telegram-shaped identity leak below the adapter boundary.
Depends: nothing. Feeds: T20.

### T11 [ASSESS] Kernel HTTP contract review (relates #117, #118)
Read docs/kernel-http-contract.md adversarially: could a non-Rust (TypeScript) worker container satisfy this contract cleanly today? List every gap.
Depends: nothing. Feeds: T12.

## P2: Platform posture (cheap, parallel, low risk)

### T12 [BUILD] #117: package-level install and run flow
Implement per T11 findings. The consuming-product story (an external product such as Bell as a kernel-contract worker) is the primary use case.
Depends: T11.

### T13 [BUILD] #118: setup friction
As filed.

### T14 [BUILD] Repo repositioning: runtime + packages
Docs and layout only: OpenSpine is the runtime; Lyra is the reference assistant package; external products such as Bell consume the runtime. No code changes.
Depends: nothing.

### T15 [BUILD] Deployment and supply-chain CI (separate slow lane)
docker compose config with assertions (shell network internal, kernel port not host-published); cargo deny check (advisories, licenses, bans, sources); GitHub Actions pinned to immutable SHAs; x86_64 and ARM build coverage. Keep out of the fast Rust gate.
Depends: nothing.

### T16 [BUILD] Graphify out, CodeGraph in
Remove committed graphify-out from the working tree, gitignore it, record the history-size finding for a human decision on history rewriting. Initialise CodeGraph (.codegraph/ gitignored); verify it answers: callers of effect executors, reach into connectors, impact radius of ResolvedActionContext.
Depends: nothing.

## P3: Post-fit-review and demand-driven (DO NOT START before the fit review)

### T17 [BUILD] Effect Truth Slices D-F: ActionIntent / ResolvedActionAttempt / ActionAdmission / ActionDisposition (part of #128)
Deepen action mediation per the spec, after human review of T1-T3.
Depends: T3, human review.

### T18 [BUILD] #128: resolved-context matching, overlap handling, drift checks
On top of T17, per the review's guidance that #128 should deepen the existing boundary, not wrap it.
Depends: T17.

### T19 [BUILD] #131: second protocol = WhatsApp Cloud API connector
The WhatsApp connector is the materially different second protocol that proves the resolver/executor seam: webhook ingress with Meta signature verification as source verification, token custody in the kernel, pre-approved template egress through the gate returning EffectDisposition, wamid recording, plus a Cloud API simulator as an in-memory adapter for tests and development. Built DURING Bell's integration spike so real requirements shape the seam; never built standalone from imagination. De-Gmail scoped_admission as part of this.
Depends: T3, integration spike running, ideally T8 verdict.

### T20 [BUILD] #129: owner surface as a genuine two-adapter boundary
Telegram and terminal (and later WhatsApp owner surfaces) render the same OwnerReviewRequest and submit the same principal-bound decisions. Shaped by T10's inventory.
Depends: T10.

### T21 [BUILD] Tenancy implementation
Implement T8's design: (tenant, principal, contact) native in identity, grants, audit. The largest single change in this roadmap; only after the fit review lands on build-on.
Depends: T8, fit review.

### T22 [BUILD] #132: whole-responsibility composition
Last, per the review: only after effect settlement (T3) and protocol conformance (T19) are stable.
Depends: T3, T19.

---

## Suggested waves for an orchestration loop
Wave 1 (parallel-safe): T0, T1, T4, T5, T14, T16, T15
Wave 2: T2 -> T3, T6, T7, T8, T9, T10, T11
Wave 3: T12, T13, re-run T7 against post-T3 main
Then: human fit review -> Wave 4 unlocks P3 in dependency order.

Filing tip: file T1-T3 as one epic with three sub-issues so a loop cannot start T2 while T1's stop condition is unresolved. Everything else is safe to file flat.