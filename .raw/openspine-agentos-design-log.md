# OpenSpine Agent-OS Design Log

Working log for the spec round that turns OpenSpine from "safety substrate + one email slice"
into the OS an agent can safely grow on. Grown iteratively from design sessions (started
2026-07-06/07); entries get amended, superseded, or promoted into openspec changes as
ambiguity clears. Distinct from `.raw/openspine-decision-log.md` (implementation decisions,
D-0XX) — entries here are AD-0XX (agent-OS design) and carry a status:

- **settled** — direction agreed in session; ready to inform a spec
- **leaning** — recommendation on the table, not yet stress-tested
- **open** — genuinely undecided

## Vision (settled)

The spec is for the OS, not a product persona. One master agent per user — the user-facing
orchestrator *within* the substrate (per D-001 the substrate is not a single agent) — unnamed,
shapeless at first, grows into whatever the user needs. The user is the president; the
agent is chief of staff. The user should almost never press "approve," and never for the
same thing twice. Guardrails invisible in daily use, but on request the agent can show
exactly how the user's interests were protected, with receipts. The system builds on
OpenSpine knowing it can do most things, and because of OpenSpine it is caught safely when
wrong, so it can fix itself without drama. Feel: collaborative, not deferential.
Reference archetype: Donna (Suits) × Leo McGarry (West Wing) — trusted operator.

## Core axes (settled)

- **AD-001 Effect axis.** Quiet learning may only shape *suggestions* (which option, ordering,
  phrasing, default time). Anything that writes to the world — calendar, reminder, message,
  send, query egress — is authority and flows through gate(), even when learned. One
  sentence: *quiet learning shapes what it says; confirmed learning shapes what it does.*
- **AD-002 Disclosure axis.** An outbound query built from private context is an effect —
  the query itself discloses ("research my condition X" sends X to a third party).
  Autonomy triage is reversibility × confidence × disclosure. Internal work (read/think/
  draft over held data) is effect-free: never ask, never wait. Mitigations: query
  generalization before egress; provenance tracking from classified briefcase items
  enables deterministic egress checks (no LLM judging "is this sensitive").
- **AD-003 Boundary rule.** *Do the work freely; ask only at the effect boundary; at the
  boundary ask only when no standing rule covers it and the effect isn't trivially
  reversible.* Waiting for permission to do effect-free work is a UX failure, not safety.

## Authority growth (settled)

- **AD-010 Standing rules.** Repeated-approval patterns become standing authority via
  agent-proposed, plain-language rules confirmed once ("you always approve appointment
  bookings — want me to just go for it? ≤5/week"). Versioned, revocable artifacts with
  expiry (e.g. lapse after 90 days unused) and drift triggers (usage pattern shifts →
  re-confirm). Weekly digest replaces per-action approval as accountability surface.
  NOTE (scope honesty): consulting standing rules at gate/compose time on live actions,
  with per-rule usage counters, is a genuinely new authority+gate path — a real slice,
  not a new enum entry. Partial machinery exists (grant_counters, budget enforcement).
  Standing rules are INPUTS to task-grant composition — per D-007 the task grant remains
  the only live authority object; a standing rule is never a separate runtime authority
  source.
- **AD-011 One-loop conversational approval.** The clarifying question IS the approval:
  "Does 2pm tomorrow work?" carries the plan digest (book + calendar + reminder + any
  data-handling steps, e.g. "scrub personal info before searching"); "yes" approves that
  digest. Generalizes the existing digest-bound approval invariant (D-011/WYSIWYS) from
  email-body-shaped to
  plan-shaped. Kills the deferential double-ask structurally.
- **AD-012 Dark-window defaults.** Time-boxed conditional grants: "if you don't respond in
  30 min, I take pre-agreed default X." A standing rule with a timer; highest-scrutiny
  audit case; needs explicit spec treatment. (From assistant-practice research.) *leaning*
- **AD-013 Autonomy calibration is learned per user.** Redo-rate (acted too soon) vs
  waited-on-obvious (asked too much) mined as the expectation signal; threshold widenings
  are standing-rule proposals like everything else.

## Memory & context (settled)

- **AD-020 Two-kinds split.** (a) Authority rules: kernel-enforced at gate, never need agent
  context, can't be forgotten or injected away. (b) Preferences: context-shaping artifacts
  packed per task. Remembering is the kernel's job, not the agent's.
- **AD-021 Briefcase packing.** Kernel packs every task's context deterministically from
  task shape (route × workflow × counterparty): grant + relevant preferences + relevant
  skills + counterparty slice. 1000 learned things, ~5 per task. No decision-DAG in prompt.
- **AD-022 Consolidation/autophagy.** Periodic miner pass merges/prunes learned artifacts
  (50 appointment micro-preferences → 3; dead rules expire). Stops "learned" = "accreted."
- **AD-023 Provenance everywhere.** Every learned artifact links to the exchange that
  produced it ("you told me March 3rd, here's the exchange").

## Delegation & containment (settled)

- **AD-030 Intern principle.** Commissioned workers are trustworthy because they don't know
  the secret, not because they're loyal. The briefcase LIMITS the blast radius for outward
  action; containment is the guarantee (D-005 shell containment, AD-034). Win by making
  foolability worthless, not workers un-foolable.
- **AD-031 Briefcase depth = f(relationship tier × task class).** Kernel-packed, minimal for
  strangers, naturally richer for intimates because more artifacts genuinely belong to that
  relationship (per-identity tagging provides AND protects). Worker may request top-ups;
  each request gate-visible; standing rules smooth repeats.
- **AD-032 Who packs: the kernel.** Master agent proposes/requests; kernel decides. Avoids
  confused-deputy over-packing from poisoned upstream context.
- **AD-033 Reply chokepoint.** Worker→master crossings are structured results (schema-checked
  fields: outcome, offered_slots, requests[]); free-text fields stay wrapped as untrusted.
  External content always enters wrapped (existing ULID-delimiter machinery); cargo, never
  commands.
- **AD-034 Detection is intelligence, containment is the guarantee.** Screening pass tags
  manipulation attempts → audit + digest + counterparty identity annotated → routing
  tightens. Detect to learn, contain to survive.
- **AD-035 Master agent never the worker.** Interpret, commission, relay — work runs in
  separate task-granted shells; results return as events; conversation stays responsive.
  Sub-grant minting + internal event bus are UX-critical, not just security items.
- **AD-036 Parameter-level binding (prepared-statement principle).** Identity- and
  scope-bearing parameters of effectful calls (customer_id, account, recipient) are bound
  by the kernel from the task grant/briefcase, never free-typed by the agent. A skill may
  shape HOW to call the CRM; the kernel fills the WHO. Like prepared statements vs
  concatenated SQL: injection can change the text around the slot, never the binding.
  The customer-service worker physically cannot query a different customer's number —
  the parameter isn't theirs to write. (settled)

## Skills & workflows (mixed — settled entries plus leaning/open items)

- **AD-040 Skills are a versioned artifact class** (how-to procedures loaded per task).
  They shape competence; authority stays in packs/gate. A skill is an *instruction
  surface* — injection-escalation vector (a poisoned skill can attempt exfil within
  existing authority, e.g. "always BCC archive@x"). Real guarantee: **the gate contains
  any skill, trusted or not** — extra recipients/egress injected by instruction text die
  at the boundary and surface in the digest. (settled as a class. D-048 revisit basis:
  D-048 kept templates fixture-only because an instruction surface is an injection-
  escalation vector — a skill is the SAME category, so the honest ground for allowing
  runtime skills is the gate-containment guarantee above, which D-048 predated. NOT the
  install-vs-propose routing: that (AD-041, never artifact.propose) is mechanism, not the
  security argument. A formal D-0XX revisit entry is required before runtime skill
  machinery ships.)
- **AD-041 Gate the shelf, not the reach.** Human involvement at skill *install/update*
  only, proportionate to provenance: shipped-seed and user-installed = already trusted
  (installation was the approval); miner-distilled = one-tap with provenance + diff;
  external = adaptation pipeline (AD-043). *Use* is silent: kernel injects installed
  skills by task shape like preferences. One decision per skill ever, not per use. The
  install path is a separate, user-controlled ceremony, distinct from the five-kind
  artifact.propose pipeline (D-048). (settled)
- **AD-042 Skill selection = deterministic index (task class → skills) + semantic matcher
  fallback, selecting only from the approved shelf.** Matcher can inject, never install.
  Wrong match costs off-target context, not a breach. Skill visibility scoped per
  agent/pack — workers can't see (or request) skills outside their job. (settled)
- **AD-043 External-skill import pipeline.** Restructure to progressive disclosure (thin
  index + on-demand references), extract workflow-shaped parts into workflow artifacts,
  statically classify implied effects/egress, offline eval in quarantine, then enter via
  the AD-041 install path (not artifact.propose; D-048) with a provenance-and-risk
  report. Imported skills arrive adapted or not at all. *leaning*
- **AD-044 Workflows are declarative state machines** (YAML, mermaid-renderable): states,
  transitions, agentic vs deterministic steps, escalation points, and **approval semantics
  mapped on workflow states**. The deterministic spine of a conversation; agent fills
  semantic gaps within states. Small models perform better with explicit state; evals
  target stages; miner crystallizes recurring ad-hoc sequences into workflow proposals. (settled)
- **AD-045 Representational thesis (to stress-test):** workflows carry deterministic shape +
  approval semantics; skills carry how-to knowledge; routing binds tasks to both. Most
  behavior should be expressible this way. *open — needs adversarial testing before it hardens*
- **AD-046 Effort routing (LOD principle).** Workflow steps may declare required reasoning
  depth; the kernel assigns model/effort tier per step (cheap model for gather-order-number,
  strong model for compose-difficult-reply) — like level-of-detail in game engines: spend
  compute where fidelity matters. Also applies to the reflection tiers (AD-050). *leaning —
  pending cross-domain research*

## Reflection & improvement (settled)

- **AD-050 Two-tier reflection.** (a) Live advisor: watches conversations in-flight,
  powerless by design (advise, never act), catches mistakes before they ship. (b)
  Scheduled systemic miner: off the live path, best affordable model, accumulated
  correction/flag backlog as its entire context; one job — "what class of problem is this
  and what artifact change handles the class?" Systemic perspective is a scheduled job,
  not a hoped-for talent. Fixes the first-order-fix problem; small models handle the live
  path.
- **AD-051 Advisor scoping rule.** An advisor never has broader *data* access than the agent
  it advises (else it's a covert channel around the briefcase). Advisors may carry richer
  *procedural* knowledge (scam patterns, policy shapes) — judgment, not facts. Cross-scope
  hints cross as structured, gate-visible messages, never ambient context.
- **AD-052 Proactivity is a budgeted lane, not a personality trait.** Hard budget (model
  calls/day, suggestions/week), provenance required (the pattern + sources), user reaction
  mined (engaged/ignored/annoyed; 5 ignores retires the class). Slop = unbudgeted,
  feedback-free proactivity.
- **AD-053 Miner output classes.** Corrections-with-reasons ("how come 2pm doesn't work?" —
  one stated reason worth fifty silent approvals), repeated approvals (standing-rule
  candidates), stated preferences. All land as proposals in the artifact lifecycle;
  nothing authority-bearing quiet-activates (AD-001).
- **AD-054 Positive-steering principle.** Corrections REWRITE instructions; they never
  append prohibitions. Owner says "don't do X" → the miner produces a tighter positive
  statement of the way ("draft replies in ≤3 sentences"), not a growing don't-list. If the
  rewrite fails, tweak the rewrite. Negative constraints live as EVAL PROBES (testable
  scenarios, e.g. the AD-081 anti-patterns), not as prompt text. Rationale: token
  efficiency, reliability, and the observed meta-failure that agents fix problems by
  appending rules — codified here as core improvement-machinery guidance because no agent
  does this by default. (settled — owner-stated core guidance)

## Egress & connectors (settled)

- **AD-060 Egress endpoints are typed and policy-rated** in the connector registry: no-log
  search API ≠ forum browse ≠ web-form POST. Packs reference egress classes ("may query
  search-class with generalized queries; may never submit forms"). Kills a category of
  in-the-moment gray-area judgment.

## Base/overlay & updates (settled)

- **AD-070 Namespacing.** Base = kernel + shipped artifacts (upstream-owned, versioned).
  Overlay = everything learned (user-owned, survives updates untouched). Update runs a
  compat pass; orphaned learned artifacts get one-tap re-confirmation — never silently
  broken, never silently kept.
- **AD-071 Direction of the line: mechanisms flow up, rules stay home.** Consolidation lane
  may nominate generalized, de-personalized patterns as upstream candidates — explicit
  user opt-in, through normal spec/review. Personal state never auto-flows upstream.

## Personality seed (settled — validated by sentiment cross-check 2026-07-07)

- **AD-080 Ship opinionated: Donna × Leo.** Eight elements from archetype+practice research
  (anticipatory provisioning; bounded autonomy; one-loop confirmation; radical context
  curation; discreet information discipline; honest counsel with recommendation; provenance
  & receipts; composed operational continuity) as pre-populated learnable artifacts in the
  overlay — day one Donna-ish, year one the user's. Not kernel-baked.
- **AD-081 Anti-patterns as testable negative constraints:** deferential double-asking,
  sycophancy, over-explaining, nagging, presumptuous anticipation, need-to-know failure,
  apology theater (when wrong: correction, root cause, preventive change — no remorse
  performance). Eval scenarios probe each.
- **AD-082 Digest/brief triage:** ≤3 priority items, sorted decisions-needed → FYI →
  handled, one line each, detail behind the fold. *leaning*
- **AD-083 Sentiment validation (ArchetypeSentimentCheck).** Fan/EA community sentiment
  matches the owner preference map point-for-point (r/suits: "we made partner" equal-
  partnership; r/thewestwing: Margaret-signature = proxy admirable only because guardrails
  explicit; r/ExecutiveAssistants: receipts as survival skill). Additions it forces:
  (a) gatekeeping is a praised *service* — protecting the principal's attention is a
  first-class function, not obstruction; (b) proxy-with-receipts: contributions must be
  legible (retrievable record of what/why) but never self-promotional; (c) anticipation
  must be pattern-with-receipts, never psychic — real EAs' top complaint is mind-reading
  expectations; (d) briefing culture IS the interface (reinforces AD-082).
  Design risks flagged: don't over-index on warmth/wit — trust through competence and
  reliability, no forced banter or faked intimacy (the "season-3 Donna" warning: pushing
  personality drama diluted the competence fans admired). New anti-patterns for AD-081:
  faked intimacy; info-dump without synthesis; self-promotional visibility.


## Task & commitment management (settled)

- **AD-090 The task board is kernel state, not agent context.** Tasks/commitments are
  first-class runtime objects (status, owning worker/grant, due, dependencies, provenance:
  "you asked Tuesday" / "promised supplier reply by Friday"), tracked deterministically.
  The master agent interprets intents into task objects and consults *slices* (briefcase
  principle applied to workload: due-now + blocked + asked-about — 20 tasks in flight ≠
  20 tasks in context). Time is an event source: deadlines/reminders fire as kernel events
  through the normal pipeline (routed, granted, gated), handled by workers — never by the
  master agent's memory. The AD-082 brief (format still leaning) is the board's read-model:
  the president sees
  the morning briefing, never the board. Third instance of the core law: remembering
  (AD-020), deciding (gate), and tracking are all the kernel's job. Validates the CoS
  "air-traffic controller" framing from the practice literature; the Hermes-kanban
  instinct was right, its placement (agent context) wrong.

## Kernel readiness (from 2026-07-06 three-reviewer deep-dive)

Invariants sound (deny-by-default, digest approval, kernel/shell split, pure
authority/gate crates); extension points concrete and closed. Refactor program, ordered:

1. Registries over match-arms: Connector trait+registry, ActionHandler registry,
   ProposableArtifact kind registry; fail-fast unknown ActionIds.
2. Pipeline driver: typed stage sequence, lanes as data (current flows = first two lanes).
3. IdentityResolver seam + identity tables (resolve_owner_identity becomes fast-path).
4. gate() integrity: enumerate trusted-path carve-outs; internal effects through gate with
   KernelOrigin (exempt from approval, never from audit); selection-token validation moves
   into gate.
5. Grant chain: parent_grant_id + attenuation (schema now, runtime later). AMENDED by
   AD-101: prefer Macaroons-style caveat chains over hand-written intersection logic.
6. Grant mode field (live/shadow) reserved in schema now — the one non-retrofittable
   evolution hook worth paying for early.

Non-retrofittable set (spec before implementation proceeds): effect-suppressed execution
mode; generation/lineage model (distinct from artifact version u32); eval-verdict/fitness
store (indexed tables, not audit chain); sub-grant attenuation; identity store +
per-identity scoping; kernel-amendment lane (openspec ceremony already serves for code;
runtime blue/green deferred until a second deployment exists).

Corrections recorded during review: store migrations DO exist (`store/migrations.rs`,
ad-hoc idempotent ALTER TABLE on every open; SCHEMA_SQL CREATE TABLE IF NOT EXISTS also
runs every open) — versioned PRAGMA user_version framework only needed when a spec first
requires retypes/backfills. Standing-rule gate-time consultation is new kernel work
(see AD-010 note).

## Resilience & runtime substrate (from ResiliencePatterns research, 2026-07-07)

- **AD-100 Supervision with authority reset.** OTP-style supervision for workers, with one
  strict deviation from let-it-crash: a restarted worker NEVER auto-inherits the dead
  worker's grant. Grants are transient, non-transferable, per task instance; on crash the
  supervisor emits a structured `worker_failed` event and continuation requires
  re-composition through the normal pipeline. Restart-intensity caps per connector
  (e.g. 3/30s) so a flaky external service can't cause a restart storm. (settled)
- **AD-101 Sub-grants as caveat chains (Macaroons-style).** Each sub-grant derives from its
  parent by ADDING caveats (action, recipient scope, model tier, expiry); the gate verifies
  the chain offline via HMAC without DB lookups. Provable monotonic narrowing — the caveat
  chain IS the attenuation proof AND the audit evidence; also the formal mechanism for
  AD-036 parameter binding (bound params are caveats). D-007 anchor: a sub-grant is still
  a task grant — it remains the ONLY live authority object presented to the delegated
  worker; the parent grant is lineage, never a second live authority source. Start
  Macaroons-simple; move to Biscuit-style Datalog only if policy expressiveness demands.
  Needs: short expiries or a revocation list. *leaning — supersedes hand-written
  attenuation-by-intersection*
- **AD-102 Identity-based worker addressing (virtual-actor pattern).** Workers/conversations
  addressed by identity tuple (owner, conversation, task), never by process handle; one
  message at a time per conversation (race-free briefcase/counter updates). In-process
  actor registry first, but identity addressing from day one — clustering must not be a
  rewrite. *leaning*
- **AD-103 Connector health in the gate path.** Per-connector circuit breaker
  (Closed/Open/HalfOpen) in the connector registry; gate blocks effects through an Open
  connector with a `connector_unavailable` audit event — operational failure is distinct
  from policy denial (matters for UX, debugging, and honest digests). (settled — bounded
  change, operationalizes AD-060.) Bulkhead resource pools: principle noted, machinery
  DEFERRED — they're a multi-tenant/high-throughput isolation pattern that contradicts the
  current single-owner design (store is deliberately mutex-serialized; D-020's
  single-owner, deployment-agnostic scope); at n=1 "hung connector freezes others" is
  solved by per-call timeouts on connector
  calls, which the circuit breaker needs anyway.
- **AD-104 Durable workflows by replay (DBOS-style, not Temporal-style ops).** Workflow
  executions are deterministic state machines; every outside-world step (model call,
  connector call, approval) records its result as an event; crash recovery = rehydrate and
  replay from the ledger. Step results in the existing store alongside the audit chain —
  no separate orchestrator. Hard requirement: workflow code deterministic; randomness/time/
  external calls kernel-mediated and recorded. Also carries AD-012 dark-window timers.
  (settled)
- **AD-105 Event bus = event-sourced audit store with typed subscriptions.** No separate
  broker: events append to the ledger BEFORE consumers act; consumers (master, advisor,
  miner, gate feedback) subscribe to filtered streams. Requirements: unique event IDs +
  per-aggregate sequence numbers (idempotent consumers). Rebuildable CQRS-style projections:
  principle noted (don't design state that CAN'T be rebuilt from the stream), machinery
  deferred — no projection framework at n=1. (settled, scale-parts deferred)
- **AD-106 Standing-rule budgets as usage plans.** Quota (volume, 5/week) distinct from
  rate (velocity, 1/hour); sliding-window counters checked at GATE time in the same
  transaction as the decision (failed effects don't consume budget); remaining budget
  returned in the gate response so agents self-adjust without extra round-trips — also
  surfaces the AD-013 calibration signal. (settled — concretizes AD-010's new-kernel-work
  note)

## Adversarial review (from AdversarialAgentLit research, 2026-07-07)

- **AD-110 Adversarial passes at promotion points only.** Mandatory for: skill
  install/update, workflow promotion (especially approval-semantics changes), standing-rule
  proposals, external imports (strictest). Never per-use — routine routing/packing/gate
  calls are defended deterministically; audit them, don't debate them. Cost logic:
  promotion events are low-frequency/high-blast-radius; execution is the reverse. (settled)
- **AD-111 Risk judge = sneaky prover.** Promotion review runs an adversarial agent that
  produces CONCRETE attack traces attempting disallowed effects within granted authority
  (prover-verifier, Kirchner 2024, arXiv:2407.13692); verdicts land in the eval store.
  Judge independence: different model family from the proposer (shared bias, Shi 2024);
  randomized presentation order, multiple passes, calibration set of known exploits. A
  weaker judge suffices when evidence is legible (Khan 2024, arXiv:2402.06782). *leaning*
- **AD-112 Live advisor = legibility checker, not self-correction oracle.** Intrinsic
  self-correction without external signal degrades output (Huang 2023, arXiv:2310.01798).
  The advisor flags missing checkable reasoning, underspecified effect bindings, misapplied
  standing rules — structured objections (concern class + cited clause + suggested
  rewrite), never "better answers" from the same context. Small/fast model live;
  spot-checked offline by an independent stronger model. (settled — refines AD-050)
- Literature anchors: the half-remembered paper = Du et al. 2023 multiagent debate
  (arXiv:2305.14325). Caution: debate among like models can DECREASE accuracy via
  conformity (Wynn 2025) — the adversarial protocol and model diversity are load-bearing;
  consensus is not the goal.

## Game-AI patterns (from GameAiPatterns research, 2026-07-07)

- **AD-120 Gate speaks intents (authoritative-server model).** The shell sends INTENTS
  ("reply to selected thread via bound recipient"), never fully-formed effects; the kernel
  computes/validates outcomes and re-derives digests from grant+payload rather than
  trusting agent-supplied digests. Netcode's lesson made design language: cheating is
  structurally impossible, not merely detected. (settled)
- **AD-121 Briefcase = kernel-owned blackboard with visibility classes.** Keys typed
  kernel-bound / worker-scratch / returned-output; a fog-of-war visibility schema records
  what each worker can see; advisor hints enter as structured gate-visible messages, never
  ambient context (operationalizes AD-021/AD-031/AD-051). (settled)
- **AD-122 Effort router = LOD-trader formulation, eventually.** Workflow steps declare
  compatible model tiers + costs; kernel maximizes quality-weighted-by-criticality within
  lane budget (criticality: user attention + proximity to effect boundary). n=1 start:
  static tier map per step; knapsack machinery deferred until budgets bite. *leaning —
  concretizes AD-046*
- **AD-123 Task-board attention scoring with hysteresis (aggro-table pattern).** Slice
  selection scored by deadline proximity, explicit user signal, blocked-on-external,
  counterparty waiting, engagement — with a hysteresis band so the active task holds the
  floor unless clearly beaten (no thrash). Concretizes AD-090's slice. *leaning*
- **AD-124 GOAP boundary.** Planners (semantic matcher, skill chaining) live in the shell
  and PROPOSE; the kernel materializes grants deterministically. A matcher choosing a
  WORKFLOW is an authority decision (approval semantics ride on it) → promotion-time
  adversarial review (AD-110) + deterministic composition. Directly informs OQ-2. (settled)

## Nerve taxonomy & channel topology (2026-07-07 brain dump, under dissection)

- **AD-130 Nerves are typed event-bus subscribers.** Every sidecar ("nerve") is declared,
  not coded ad hoc: subscription filter (which streams) × measure (what it checks against)
  × speak-threshold (the bar for interjecting) × budget × model tier × scope (≤ advisee,
  AD-051). Types so far: advisor (judgment flags), injector (skill/workflow matcher),
  screener (inbound manipulation), miner (offline systemic), meta-cognition (watches the
  conversation AND other nerves' interjections — second-order health). Prefer small,
  focused, cheap-model nerves with constrained checks over one omniscient observer.
  Substrate already exists: AD-105 typed subscriptions. *leaning*
- **AD-131 Master context is for the relationship, not the work.** Commissioned task
  detail never RESIDES in master context — the task board holds it (AD-090); completion/
  attention events bring back a slice. No "context-eviction nerve" needed if the
  dispatcher pattern is done properly; the residual is ordinary compaction, a kernel
  service. Context budget spends on personality, relationship, and the active exchange.
  (settled)
- **AD-132 Speak-threshold discipline.** A nerve that interjects on everything is noise
  (the season-3-Donna failure, applied to machinery). Bars: deterministic pre-filters
  first, severity classes, ignored-interjection decay (AD-052's retire rule generalized
  to nerves). *leaning*
- **AD-133 Escalation is the direct-contact surface (owner call 2026-07-07, resolves
  OQ-11).** A worker talks to the user directly only when it ESCALATES — confidence too
  low to continue without a human — and escalation routing is deterministic kernel
  machinery (route by task; thread binding subject to the OQ-12 decision), not a
  personality choice. Escalation routes TO the owner without collapsing the master into
  task execution — the master remains the conversational orchestrator (AD-035).
  Different tasks may route to different agents. Whether that surface reads as "the same
  agent" or "a staff member" is presentation — deliberately NOT settled at design time;
  ships as a default, learned per-user (AD-135). The office-vs-persona question dissolves
  into escalation mechanics (spec now) + learnable presentation (overlay). (settled)
- **AD-134 Headless lanes: hook-triggered workflows.** A hook (e.g. GitHub webhook →
  code-review workflow) is just another event source: verified, identified, routed,
  granted, gated — with no owner conversation anywhere in the loop when composed
  authority requires no approval. "Working machinery" runs silently; it surfaces only via
  the digest (AD-082, format still leaning) or meta-conversation ("how was the day"). How
  much surfaces is a
  learned overlay preference, not a fixed notification setting. Validates that the
  pipeline is event-shaped, not chat-shaped. (settled)
- **AD-135 Ship defaults, learn preferences (design-time restraint rule).** When a design
  question is really a user-preference question that can be safely learned at runtime
  (presentation voice, notification volume, interjection frequency), do NOT settle it in
  spec — ship an opinionated default as an overlay artifact and let the normal
  correction→miner→proposal loop converge it. Spec effort goes only to what CANNOT be
  learned safely: authority, escalation mechanics, containment. Corollary: the product
  must be usable while shaping itself — in-the-moment "this isn't working" articulation
  is a first-class improvement input (AD-053/AD-054), mirroring how this spec round
  itself is being run. (settled)
- **AD-136 Persona binding is route-resolved (kernel space).** WHICH persona fronts a
  conversation is decided deterministically at route time from (connector/number ×
  sender identity × relationship), never by the agent: the owner messaging the
  customer-service number still reaches main Donna; a new number hits cold-intake with a
  minimal persona and minimal briefcase; a repeat customer gets the customer-service
  persona pre-packed with that counterparty's context slice. Personas are overlay
  artifacts (AD-080), but their BINDING is kernel machinery — and it's the same
  mechanism as hooks (AD-134): a phone number and a webhook are both event sources with
  route tables. Identity confusion (a customer reaching the owner persona, the owner
  trapped in the CS persona) is structurally impossible because binding happens before
  any agent code runs. Schema support already exists (RouteWhen matches
  source/connector/account_role/actor.relationship). (settled)

## Observability & failure-surfacing (2026-07-07 blindspot round)

- **AD-137 Observability ground truth (code-audited 2026-07-07).** What exists: the
  hash-chained audit log IS well-instrumented on decision paths — denials, dispatch
  failures, and mutation detections all write events (`action.dispatch_failed`,
  `draft.proposal_failed`, `auth.rejected`, `draft.target_mutated_since_approval`);
  tracing goes to stdout via fmt+EnvFilter; there are ZERO metrics counters. Confirmed
  invisible-failure holes: (1) the API layer drops audit-append failures
  (`let _ = append_audit` — actions.rs:153/285/311, api/mod.rs:126/143/156) while
  most pipeline decision paths propagate with `?` (the notification path is
  intentionally best-effort — hole 2) — inconsistent, and under SQLite failure (disk full)
  a FAILED action leaves no durable trace at all; (2) the `owner.notified` audit row is
  written BEFORE the Telegram send and a send failure is only warn-logged
  (pipeline/mod.rs:138-146) — the chain can claim the owner was notified when they never
  were; (3) default log config is ERROR-only (`EnvFilter::from_default_env`,
  main.rs:49-51), so every `warn!` on a failure path is invisible unless RUST_LOG is
  set; (4) no failure-rate counters, so a connector failing 30% of the time has no
  signal short of reading raw audit rows; (5) no dead-letter/escalation when the
  notification channel itself fails. (settled — these are facts; the fix contract is
  OQ-14)

## Blindspot resolutions (2026-07-07, owner-approved: recommendations Q1-Q7 adopted)

- **AD-138 Failure-surfacing contract (resolves OQ-14).** Invariant: NO failed effect
  without (a) a durable record and (b) an owner-visible surface. Taxonomy + routing:
  authority/escalation-class failures notify the owner immediately (they are AD-133's
  surface); connector/resource-class failures batch into the AD-082 digest; a failed
  audit append FAILS the action — an effect that cannot be recorded does not happen
  (extends the D-011/WYSIWYS spirit to recording; kills the `let _ = append_audit`
  pattern, AD-137 hole 1). That fail-the-action rule governs the effect's OWN audit
  append; the owner-notification path is instead governed by the next rule: a failed
  owner notification goes to a dead-letter queue with
  retry and writes a truthful `owner.notify_failed` event — record the attempt, then
  the outcome, never "notified" before the send (AD-137 hole 2). Minimal metrics
  surface: per-connector success/failure counters as kernel tables (no external metrics
  stack at n=1, consistent with AD-103's pays-off-at-n=1 filter) — the same counters
  AD-103's breaker and AD-013's calibration signal need. Failure-path events live in
  the audit store, not stdout warns; the default log filter (AD-137 hole 3) stops
  being load-bearing. (settled)
- **AD-139 Day-2 operations contract (resolves OQ-15).** Versioned schema migrations
  (`PRAGMA user_version`) with a documented downgrade path, upgrading the ad-hoc
  idempotent ALTER lane the moment a destructive migration is first needed;
  backup/restore treats SQLite DB + artifact blobs + keys (including AD-140's
  per-counterparty payload keys) as ONE consistent snapshot
  set with a documented restore drill; disk-full degrades loudly — actions fail per
  AD-138 rather than running unrecorded; timestamps and timeout logic trust the wall
  clock (NTP assumed; chain ordering is append-order), with clock-regression detection
  at boot. Runtime blue/green kernel trial stays deferred (kernel-readiness note,
  AD-070 discipline). (settled)
- **AD-140 Crypto-erase for counterparty deletion (resolves OQ-7).** Private payloads
  encrypted under per-counterparty keys; erasure = key deletion — plaintext becomes
  unrecoverable while the hash chain keeps its tamper evidence intact. Derived overlay
  artifacts (rules/preferences mined from erased conversations) are invalidated via
  their provenance links — provenance discipline (AD-023, AD-002) is what makes erasure
  propagation computable. Specified before first production deployment, not patched in
  later. (settled)
- **AD-141 Connector reality contract (resolves OQ-16).** The kernel owns per-connector
  rate-limit buckets with backoff and refresh-before-expiry token handling (never let a
  Gmail token lapse mid-task); AD-134 hook lanes require webhook signature
  verification, idempotency keys, and bounded replay windows — a spoofed or replayed
  webhook is an attack path, handled structurally, not heuristically. Complements
  AD-103: breakers are health; this is admission control + authenticity. (settled)
- **AD-142 Overlay eval gate (resolves OQ-17).** Every authority-bearing proposal
  (standing rules foremost) passes offline replay against captured owner history plus
  an adversarial risk-judge pass (AD-110/111) BEFORE reaching the owner's approval tap;
  results attach to the proposal as evidence, so the one-loop confirmation (AD-011) is
  informed, not decorative. Quiet-activating preference suggestions keep the lighter
  bar (AD-001's effect-axis split). (settled)
- **AD-143 Global spend kill-switch (resolves OQ-8's cost half).** A global per-day
  spend cap across all model calls and connector usage sits above per-task grant
  budgets (AD-106, AD-122); breach pauses proactive and headless lanes and notifies
  the owner immediately — wallet-draining is urgent by definition, so it rides the
  immediate lane of AD-138's taxonomy, not the digest. Latency half of OQ-8 (per-stage
  ms budgets) remains open. (settled)
- **AD-144 First-run & multi-device posture (resolves OQ-18 at current depth).** The
  bootstrap sequence is documented as-is: env-var config (D-014), Telegram-first owner
  control (D-030) — bot token → owner verification → Gmail OAuth — including its
  failure messages. Two simultaneous owner devices are ONE identity; races on the SAME
  conversation are serialized by AD-102's one-message-per-conversation rule, while
  cross-conversation device consistency stays subject to OQ-12/AD-102 follow-up.
  Richer onboarding UX deferred until a second deployment exists to learn from.
  (settled)

## Interview resolutions (2026-07-07, owner-ratified; adversarially debated per rule)

Decisions taken in an interview-me round (one question per turn, blast-radius order),
then stress-tested by a reformer-vs-conservative subagent debate (DecisionReformer,
DecisionConservative) before landing. The debate corrected one decision's scoping
(AD-148) and converted assumptions into explicit prerequisites throughout.

- **AD-145 Spec everything; order is the loop's concern (resolves OQ-1).** The
  deliverable is the complete agent-OS spec corpus; implementation order is delegated
  to the dev loop as a scheduling annotation, never an architectural argument.
  Guard (from debate): entries whose feasibility rests on unbuilt substrate carry
  explicit prerequisites; the loop MUST NOT promote design prose to a requirement
  past an unmet prerequisite, and every slice proposal is checked against the kernel
  invariants (deny-by-default D-004, identity-is-not-authority D-006, grant-is-only-
  live-authority D-007, digest-bound approval D-011, kernel/shell split D-005/D-010,
  deterministic routing D-008). (settled)
- **AD-146 Principal-shaped schema, single-owner v1 (resolves OQ-4; OQ-3 at bootstrap
  depth).** `Principal` becomes a first-class record; v1 enforces exactly one
  (`is_owner`); the owner stops being a config string (today the pipeline wires
  `state.owner_user_id` straight into composition — pipeline/mod.rs:373); identity
  resolution returns a `principal_id`. `Identity` keeps ZERO authority fields (D-006).
  Relationship-scoped disclosure: a `DisclosurePolicy` artifact keyed
  (relationship × disclosure-class), acquired lazily — a counterparty hits a
  deterministic block, the owner answers one honest question (AD-133), the answer
  becomes a standing rule with carve-outs. Identity bootstrap is owner assertion
  ("my wife's number is this") as an owner-approved, AUDITED action — never
  agent-triggered, or the agent could mint relationships and thus authority.
  Unknown claims never auto-bind (RelationshipKind::Unknown, confidence 0).
  A counterparty with rich standing rules is still not a principal; the principal
  seam means promotion later is additive, not a rewrite. (settled)
- **AD-147 Matcher confined to authority-equivalence classes (resolves OQ-2).** The
  kernel — never the shell — computes equivalence classes deterministically from
  artifacts' DECLARED action lists: two candidates are in one class iff their composed
  (allowed_actions, approval_required_actions, denied_actions, output_channels,
  limits) are identical (shapes verified comparable: grant.rs:54-61). The semantic
  matcher picks freely WITHIN one class (taste, not authority) and never across;
  cross-class ambiguity resolves by deterministic rule or escalation. This REFINES
  D-008, not repeals it: the LLM still never resolves route conflicts or constructs
  grants; within-class members are authority-identical by construction, so the pick
  cannot widen anything. Classes are auditable and testable, never LLM-derived.
  Prerequisites: validated ActionId registry (kernel-readiness refactor 1 — ActionId
  is an unvalidated string today). Matcher model swaps pass the AD-142 replay gate;
  v1 uses off-the-shelf LLMs; the authority-free slot is the first candidate for a
  small OSS model later. (settled)
- **AD-148 Thread↔grant binding — channel-agnostic, dormant in v1 (resolves OQ-12;
  corrected by debate).** `EventEnvelope` and `TaskGrant` gain optional `thread_id`;
  binding is KERNEL-owned: a reply in thread T resolves to the grant bound to T —
  deterministic, the same move as digest-bound approval applied to conversation; no
  binding → master thread; a worker replies only in its bound thread and escalates
  only to the master thread (AD-133); the shell never creates or switches threads.
  Debate correction: Telegram topics are a GROUP-only feature and owner control is
  deliberately private-chat-only (telegram.rs verify_update requires is_private_chat),
  so the binding lies dormant until a thread-capable channel (e.g. Discord) ships;
  putting owner control in a Telegram group would require re-threat-modeling
  group-visible authority and is out of scope. (settled)
- **AD-149 Miner is a worker role — layered (resolves OQ-5).** v1 reflection runs
  under an ORDINARY task grant: scoped audit-trail-slice briefcase, explicit grant
  fields (classification ceiling, empty output_channels, model-call/artifact limits),
  model calls through the gateway and gate like everyone else; outputs are proposable
  artifacts through the normal lifecycle, never direct mutation of kernel state or
  standing rules; NEVER a privileged background daemon — that would be a covert
  channel around gate/budget. Generalization to N narrow nerves (typed event-bus
  subscribers, AD-130; one smart miner vs many narrow ones over the same stream)
  has PREREQUISITES, not co-requisites: the AD-060 connector registry and the AD-105
  event bus must exist first. More miners ≠ more authority: N miners only generate more
  proposals for the same one-tap approval; AD-132 speak-thresholds gate the noise.
  (settled)
- **AD-150 Overlay export/restore — first-class, key-model gated (resolves OQ-6).**
  The learned overlay IS the relationship; export/restore are owner-only gated
  actions. Bundle = SQLite DB + artifact blobs + key material as ONE atomic snapshot
  (AD-139 scope) with a documented restore drill; base-version migration runs the
  AD-070/071 compat pass. HARD PREREQUISITE (from debate): the artifact store must
  migrate from its single global AES key (artifact_store.rs) to per-counterparty
  payload-key derivation (AD-140) BEFORE any export/restore is claimed — otherwise
  the single-key design gets baked into the export format and crypto-erase becomes a
  re-encrypt-everything migration. (settled)
- **AD-151 Refusals never leak policy (resolves OQ-9).** Gate denials stay enum
  reason codes (action.rs enum, used in gate.rs); workers receive outcomes, not
  policy text.
  The spec ships ONE canonical policy-free refusal ("I need to check on that — I'll
  get back to you") plus deterministic escalation to the owner (AD-133). Phrasing is
  learnable presentation; the no-leak invariant is kernel. "I'm not allowed to
  discuss X" is itself a disclosure. (settled)
- **AD-152 Model swap is a ceremony (resolves OQ-10).** Personality seed and overlay
  are artifacts, not weights — so a base-model swap is a PROPOSAL carrying golden-set
  replay evidence through the AD-142 gate (replay + adversarial risk judge),
  digest-approved by the owner; no silent swaps. The spec defines the golden-set
  format and pass/fail criteria. Applies equally to matcher (AD-147) and miner
  (AD-149) models. (settled)
- **AD-153 Seed workflows: minimal, overlay-shipped (resolves OQ-13).** Seed set is
  only what current slices imply — owner-control conversation, selected-thread email
  draft with approval, research-and-brief — plus the customer-service intake template
  as the stress test; shipped as overlay artifacts, never kernel fixtures (AD-071,
  AD-080 precedent). Everything else arrives via miner proposals. (settled)

## Open questions

- **OQ-1 Forcing use case / slice order.** People-first (identity store + outbound +
  standing rules) vs evolution-first (eval harness + lineage). Session lean: people-first —
  the customer-service scenario justifies per-identity scoping early.
  RESOLVED by AD-145 (2026-07-07): spec everything; order delegated to the dev loop.
- **OQ-2 Skill/workflow matching authority.** How much routing may the semantic matcher do
  before it needs the same determinism discipline as route resolution (PRD forbids LLM
  route-conflict resolution)? Where's the line between "inject a skill" (harmless) and
  "choose a workflow" (approval semantics ride on it)?
  RESOLVED by AD-147 (2026-07-07): kernel-computed authority-equivalence classes.
- **OQ-3 Counterparty verification.** How does the system establish "this number IS my
  dentist" initially? Identity bootstrapping trust, spoofing resistance.
  RESOLVED by AD-146 (2026-07-07) at bootstrap depth: owner assertion (audited,
  owner-approved); unknown claims never auto-bind; verification beyond assertion
  deferred.
- **OQ-4 Multi-principal households.** Spouse/family as principals (not just counterparties):
  whose overlay, whose authority, whose data when both talk to "the" agent?
  RESOLVED by AD-146 (2026-07-07): principal-shaped schema, single-owner v1.
- **OQ-5 Miner privilege & locality.** The reflection miner reads everything — most
  privileged component in the system. Which model runs it, does its context ever leave the
  box, how is IT contained?
  RESOLVED by AD-149 (2026-07-07): miner is a worker under grant/gate/ceilings.
- **OQ-6 Overlay backup/portability.** The learned overlay IS the relationship; losing it =
  losing the assistant. Export/restore/migrate semantics.
  RESOLVED by AD-150 (2026-07-07): atomic bundle, gated by the AD-140 key-model
  migration.
- **OQ-7 Retention & deletion.** Counterparty data rights (customer asks to be forgotten);
  what the audit chain's immutability means for erasure obligations. Blindspot-round
  option on the table: crypto-erase — per-counterparty payload keys, delete the key to
  erase content while the chain keeps its tamper evidence; derived overlay artifacts
  (learned rules mined from erased conversations) must be invalidated too. Needs
  specifying before first production deployment, not as a compliance patch.
  RESOLVED by AD-140 (2026-07-07): crypto-erase adopted.
- **OQ-8 Latency budget.** Packing + gate + wrapping must not make the assistant feel slow;
  what's the ms budget per stage? Cost twin (blindspot round): a global per-day spend
  cap / kill-switch beyond per-task budgets (AD-106, AD-122), so a runaway loop or
  popular skill can't empty the owner's wallet.
  Cost half RESOLVED by AD-143 (2026-07-07); latency ms budgets still open.
- **OQ-9 Gate-block UX.** When the gate blocks mid-conversation with a counterparty, what
  does the worker say — without leaking policy detail ("I'm not allowed to discuss X" is
  itself a disclosure)?
  RESOLVED by AD-151 (2026-07-07): neutral deferral, no-leak invariant, escalation.
- **OQ-10 Model-swap consistency.** Seed personality and learned style must survive changing
  the underlying model; what's tested at swap time?
  RESOLVED by AD-152 (2026-07-07): swap = proposal + golden-set replay evidence.
- **OQ-11 One persona or an office?** RESOLVED by AD-133 (2026-07-07): dissolved into
  deterministic escalation surfaces + learnable presentation. Neither pole chosen at
  design time; office-vs-persona was the wrong axis.
- **OQ-12 Thread-per-task channel binding.** If interactive tasks get dedicated threads,
  the thread ID binds to the task grant — deterministic routing per thread, worker
  outbound scoped to its own thread (parameter binding applied to channels). Open: which
  tasks qualify (workflow declares interaction mode?); what surfaces in the main thread
  vs the task thread.
  RESOLVED by AD-148 (2026-07-07): kernel-owned thread↔grant binding,
  channel-agnostic and dormant until a thread-capable channel exists.
- **OQ-13 Seed workflows.** Which workflows, if any, ship as seeds vs. being mined
  per-user? (Dropped brain-dump thread, recovered.) Tension: the anti-pre-baking
  principle (mechanisms flow up, rules stay home, AD-071) vs. the fact that the existing
  email-draft + approval flows already function as seed workflows, and the personality
  seed (AD-080) sets precedent: ship opinionated starting artifacts in the overlay,
  learnable/divergent from day one. Likely answer is the same pattern — a small seed set
  (owner-control conversation, draft-with-approval, research-and-brief) as overlay
  artifacts, not kernel fixtures — but undecided and unscoped.
  RESOLVED by AD-153 (2026-07-07): minimal seed set as overlay artifacts.
- **OQ-14 Failure-surfacing contract.** The product thesis makes silent competence the
  default (AD-134 headless lanes) — which makes silent failure the top operational risk.
  Candidate invariant: NO failed effect without (a) a durable record and (b) an
  owner-visible surface. Open: the failure taxonomy (authority vs connector vs resource
  exhaustion), which classes notify immediately vs digest-only (AD-082), what happens
  when the audit append or the notify channel ITSELF fails (dead-letter + retry vs fail
  the action), and the minimal metrics surface (per-connector failure counters feed
  AD-103's circuit breaker anyway). AD-137 holds the code evidence.
  RESOLVED by AD-138 (2026-07-07): taxonomy + fail-the-action + dead-letter + counters.
- **OQ-15 Day-2 operations contract.** Upgrade/rollback (versioned migrations + a
  documented downgrade path — the ad-hoc idempotent ALTER lane exists in
  store/migrations.rs but there's no `PRAGMA user_version`), backup/restore scope
  (SQLite + artifact blobs + keys as ONE consistent set), disk-full behavior, and
  clock-skew tolerance (token expiry, breaker timeouts, and audit timestamps trust
  the wall clock; chain ordering itself is append-order). Not covered anywhere; AD-070
  covers only overlay compat.
  RESOLVED by AD-139 (2026-07-07).
- **OQ-16 Connector API realities.** Per-connector rate-limit buckets, token refresh
  before expiry (Gmail OAuth), and — for AD-134 hooks — webhook signature verification,
  idempotency keys, and replay windows: a spoofed or replayed webhook is a direct
  attack path. AD-103 covers circuit breaking only.
  RESOLVED by AD-141 (2026-07-07).
- **OQ-17 Overlay eval strategy.** Continuous evaluation of learned rules/preferences
  BEFORE activation: offline replay of past owner conversations vs a holdout set;
  whether every standing-rule proposal gets an adversarial risk-judge pass
  (AD-110/111). Miner outputs already flow as proposable artifacts; the test gate for
  them is unspecced.
  RESOLVED by AD-142 (2026-07-07): replay + risk-judge before the tap.
- **OQ-18 First-run & multi-device.** The onboarding sequence (env-var bootstrap D-014,
  Telegram-first D-030: bot token → owner verification → Gmail OAuth) with its failure
  messages, and whether two simultaneous owner sessions (phone + desktop Telegram) race
  on briefcase/counter updates or are serialized by AD-102's one-message-per-
  conversation rule.
  RESOLVED by AD-144 (2026-07-07) at current depth: bootstrap documented, AD-102
  serializes same-conversation races; richer onboarding deferred.

## Session provenance

Sources: design sessions 2026-07-06/07 (this log's origin); three-reviewer kernel deep-dive
(KernelArchReview, RuntimeSemanticsReview, EvolvabilityGapAnalysis); assistant-archetype
research (AssistantArchetypeResearch); archetype sentiment cross-check
(ArchetypeSentimentCheck, completed 2026-07-07, folded into AD-083); cross-domain research
(GameAiPatterns → AD-120s, ResiliencePatterns → AD-100s, AdversarialAgentLit → AD-110s);
pre-commit review (LogReviewer, findings applied 2026-07-07). Original vision lineage:
PRD v3 research (germline/somatic, three-lane evaluator, autophagy) — conversation
artifacts, first captured in-repo here.
Blindspot round 2026-07-07: kernel observability audit (ObservabilityAudit) and spec
blindspot pass (BlindspotPass) — the latter run via the finding-unknowns skill pack
(Thariq Shihipar's unknowns framing, distilled by Neeeophytee/finding-unknowns-skills),
vendored repo-scoped at `.omp/skills/` in this round; AD-137, OQ-14..18, and the OQ-7/
OQ-8 amendments captured from it.
Resolutions AD-138..144 owner-approved 2026-07-07 (blindspot recommendations Q1-Q7
adopted as given).
Interview round 2026-07-07: five architecture-changers + four tail defaults settled
one question per turn (interview-me skill); AD-145..153. Pre-canon adversarial debate
(DecisionReformer vs DecisionConservative, per decision-fork rule) corrected AD-148's
channel scoping and surfaced the AD-150 key-model prerequisite; grant-shape
comparability for AD-147 verified against grant.rs directly.
