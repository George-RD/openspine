# OpenSpine OpenSpec change sequence

This sequence decomposes the PRD and the agent-OS design canon into reviewable
OpenSpec changes. Requirement CONTENT lives in the canon sources below; this file
holds only the decomposition: change identifiers, dependency edges, scope
boundaries, and completion criteria. On any conflict, the canon sources win.

## Canon sources

- `.raw/openspine-agentos-design-log.md` — AD-0XX agent-OS design entries. Only
  entries marked **settled** bind a spec. Where a brief cites an entry marked
  *leaning*, that entry is the default the change proposal starts from; refining
  or replacing it requires a D-0XX decision-log entry. Entries marked *open*
  (currently AD-045) never enter a spec.
- `.raw/openspine-decision-log.md` — D-0XX implementation decisions.
- `.raw/openspine-prd-v9.md` — product frame (already decomposed by the
  completed changes below; the agent-OS sequence draws on the AD log).

## Completed / archived

- `define-openspine-development-process`
- `define-core-runtime-schemas`
- `implement-authority-composition`
- `implement-gate-action-api`
- `implement-telegram-owner-control-slice`
- `implement-selected-thread-email-preview-slice`
- `implement-digest-bound-draft-approval`
- `backfill-implemented-capability-specs` (retroactive specs for
  `implement-model-gateway`, `implement-audit-artifact-store`,
  `implement-shell-containment`)
- `harden-approval-and-budgets`
- `implement-artifact-lifecycle-slice`
- `refactor-kernel-registries` (D-053; first change of the AD sequence below —
  its brief stays in place because later `Requires:` lines reference it)
- `refactor-pipeline-driver` (D-054; its brief stays in place for the same
  reason)
- `harden-gate-trusted-paths` (D-055; its brief stays in place for the same
  reason)
- `define-grant-chain-and-modes`
- `implement-identity-store-and-principal`
- `define-lineage-and-eval-store` (D-056; brief stays in place — later
  `Requires:` lines reference it)
- `implement-event-bus-subscriptions` (brief stays in place — later
  `Requires:` lines reference it)
- `implement-egress-classes` (brief stays in place — later `Requires:` lines
  reference it)
- `implement-plan-digest-approval`
- `implement-escalation-and-refusal` (D-057..D-059; brief stays in place —
  later `Requires:` lines reference it)
- `implement-overlay-eval-gate` (D-060; brief stays in place — later
  `Requires:` lines reference it)
- `implement-model-swap-ceremony` (D-061..D-063; brief stays in place —
  later `Requires:` lines reference it)
- `implement-secret-intake` (D-064..D-067; brief stays in place — later
  `Requires:` lines reference it)
- `implement-failure-surfacing-contract` (D-068..D-072; brief stays in place —
  later `Requires:` lines reference it)
- `implement-durable-workflow-replay` (D-073..D-074; brief stays in place —
  later `Requires:` lines reference it)

## Agent-OS change sequence (2026-07-07, AD canon)

### Loop execution contract (AD-145)

- Implementation order is the loop's concern, never an architectural argument.
  A change is **eligible** when every change named in its `Requires:` line is
  archived. Among eligible changes any pick is valid; default to the
  earliest-listed eligible brief.
- The per-brief `Requires:` lines are the ONLY authoritative dependency
  statement in this file. Edges marked **HARD** came out of adversarial debate;
  the loop MUST NOT begin such a change before its prerequisite is archived,
  and MUST NOT promote design prose to a requirement past an unmet
  prerequisite.
- Every change proposal is checked against the kernel invariants before
  implementation: deny-by-default (D-004), shell containment (D-005),
  identity-is-not-authority (D-006), grant-is-the-only-live-authority (D-007),
  deterministic routing (D-008), kernel/shell split (D-005/D-010), digest-bound
  approval (D-011).
- Cross-cutting axioms bind every proposal rather than any one brief: effect
  axis (AD-001), boundary rule (AD-003), two-kinds memory split (AD-020),
  intern principle (AD-030), detection-is-intelligence/containment-is-the-
  guarantee (AD-034), mechanisms-flow-up-rules-stay-home (AD-071),
  master-context-is-for-the-relationship (AD-131), ship-defaults-learn-
  preferences (AD-135).
- Ceremony per change: branch off fresh main → openspec proposal (delta
  requirement headers MUST be `## ADDED/MODIFIED/REMOVED/RENAMED Requirements`;
  when the target capability spec is pre-seeded, requirements that already
  exist in `openspec/specs/<capability>/spec.md` MUST be carried as
  `## MODIFIED Requirements`, never re-`ADDED` — `ADDED` is only for
  requirements genuinely absent from the canonical spec) → implement →
  `./scripts/check.sh` green → independent reviewer pass on the diff BEFORE
  commit → PR → squash merge → archive with
  `openspec archive <id> --yes` so deltas are applied into `openspec/specs/`
  mechanically, then re-run `openspec validate --all --strict`. `--yes` is
  permitted ONLY on `openspec archive` in non-interactive runs (the archive
  confirmation prompt is meaningless without a human TTY; the human gate is
  PR review) — it remains forbidden everywhere else. `--skip-specs` is
  reserved for changes with genuinely no spec impact (tooling/docs); it is no
  longer the pre-seeded-conflict workaround, and hand-applying deltas into
  `openspec/specs/` is retired — a delta must never be stranded outside the
  spec corpus (D-049), and mechanical apply plus strict validation is how
  that is guaranteed (D-052).
- Any requirement dropped or narrowed during conversion from canon gets a
  D-0XX decision-log entry (spec-debt rule; D-049 precedent).
- A discovered decomposition gap (missing edge, unowned machinery) or a newly
  settled AD gets a new D-0XX plus an amendment to this file — NEVER an
  in-flight scope stretch of a running change.
- "Done when" bullets are per-change observable outcomes; the global bar
  (gates green, strict validation, reviewer pass) applies to every change and
  is not restated per brief.
- Scope bullets paraphrase canon only to draw boundaries; they are never
  authoritative — always spec from the cited AD/D entries.

### Kernel foundation

#### refactor-kernel-registries

- **Canon:** kernel-readiness item 1; named prerequisite of AD-147.
- **Requires:** none.
- **Scope:** Connector trait + registry; ActionHandler registry;
  ProposableArtifact kind registry; validated ActionId registry — unknown
  ActionIds fail fast at composition and at gate. Behavior-preserving:
  registries replace match-arms one-to-one.
- **Done when:** adding a connector/action/artifact kind is a registration,
  not a match-arm edit; an unknown ActionId produces a structured error with a
  test; existing flows unchanged.

#### refactor-pipeline-driver

- **Canon:** kernel-readiness item 2.
- **Requires:** none.
- **Scope:** typed stage sequence
  (event→verify→identify→route→compose→grant→run→gate→audit) as a driver;
  lanes as data; current flows re-expressed as the first two lanes.
- **Done when:** stages are data-driven and existing tests pass unchanged
  (behavior-preserving refactor).

#### harden-gate-trusted-paths

- **Canon:** kernel-readiness item 4; AD-120.
- **Requires:** none.
- **Scope:** enumerate every trusted-path carve-out around `gate()`; internal
  effects go through gate with a KernelOrigin marker (exempt from approval,
  never from audit); selection-token validation moves into gate; the kernel
  re-derives digests from grant+payload, never trusting agent-supplied digests
  (AD-120: the shell sends intents, the kernel computes outcomes).
- **Done when:** the carve-out enumeration is a spec requirement with a test
  per entry; no effect path bypasses `gate()`.

#### define-grant-chain-and-modes

- **Canon:** kernel-readiness items 5–6; AD-101 (*leaning* — Macaroons-simple
  caveat chains as default); AD-036.
- **Requires:** none.
- **Scope:** schema plus gate verification semantics — `parent_grant_id`;
  caveat-chain attenuation encoding where each sub-grant only ADDs caveats and
  kernel-bound parameters (AD-036) are caveats; grant `mode` field
  (live/shadow) with shadow = effect-suppressed execution semantics defined at
  gate. Runtime sub-grant minting belongs to `implement-worker-runtime`.
- **Invariant:** a sub-grant is still a task grant — the only live authority
  object presented to a worker (D-007); the parent grant is lineage.
- **Done when:** schemas carry chain + mode; gate rejects any caveat-widening
  chain under test; shadow-mode gate behavior is specified and tested.

#### implement-identity-store-and-principal

- **Canon:** AD-146; kernel-readiness item 3; D-006.
- **Requires:** none.
- **Scope:** identity tables + IdentityResolver seam (owner resolution becomes
  the fast path); `Principal` record with exactly one owner enforced in v1;
  the owner stops being a config string — composition consumes a
  `principal_id`; owner-asserted identity binding ("my wife's number is this")
  as an audited, owner-approved action, never agent-triggered; unknown claims
  never auto-bind (RelationshipKind::Unknown, confidence 0).
- **Invariant:** `Identity` keeps ZERO authority fields (D-006); a counterparty
  with rich standing rules is still not a principal.
- **Done when:** pipeline composition consumes `principal_id`, not the owner
  config string; binding happens only via the audited owner-approval path.

#### define-lineage-and-eval-store

- **Canon:** kernel-readiness non-retrofittable set; AD-111 (*leaning* —
  cited only for verdict landing).
- **Requires:** none.
- **Scope:** generation/lineage model for artifacts (distinct from the
  version u32); eval-verdict/fitness store as indexed tables, not audit-chain
  rows.
- **Done when:** schema + store APIs exist with tests; artifact rows can carry
  lineage.

### Event substrate

#### implement-event-bus-subscriptions

- **Canon:** AD-105.
- **Requires:** none.
- **Scope:** events append to the ledger BEFORE consumers act; typed filtered
  subscriptions; unique event IDs + per-aggregate sequence numbers;
  idempotent-consumer contract.
- **Out:** projection framework (deferred per AD-105's scale note).
- **Done when:** a consumer replays a filtered stream idempotently under test.

#### implement-durable-workflow-replay

- **Canon:** AD-104; AD-012 (*leaning* — timers are the substrate dark-window
  grants ride on); D-073..D-074.
- **Requires:** implement-event-bus-subscriptions.
- **Scope:** deterministic workflow executions; every outside-world step
  (model call, connector call, approval) records its result as an event; crash
  recovery = rehydrate and replay; randomness/time/external calls
  kernel-mediated and recorded; kernel timer events (this brief OWNS the
  timer substrate; task-board and standing-rules consume it).
- **Done when:** a kill-and-recover test replays to identical state without
  re-running recorded steps.

#### implement-task-board

- **Canon:** AD-090; AD-131; AD-123 (*leaning* — ship deterministic slice
  selection; hysteresis attention scoring deferred).
- **Requires:** implement-event-bus-subscriptions;
  implement-durable-workflow-replay (scoped edge: only deadline/reminder
  firing rides on its kernel timer events).
- **Scope:** tasks/commitments as kernel objects (status, owning worker/grant,
  due, dependencies, provenance); time as an event source — deadlines and
  reminders fire through the normal pipeline (routed, granted, gated); slice
  read-model (due-now + blocked + asked-about); task detail never resides in
  master context (AD-131).
- **Done when:** a deadline fires as a routed/granted/gated event under test;
  the master receives slices, never the whole board.

### Failure surfacing & operations

#### implement-failure-surfacing-contract

- **Canon:** AD-138 (contract); AD-137 (code-audited evidence); D-068..D-072.
- **Requires:** none.
- **Scope:** a failed audit append FAILS the action (no effect without a
  durable record); the owner-notification path records attempt then outcome —
  dead-letter queue with retry and a truthful `owner.notify_failed` event,
  never "notified" before the send; failure taxonomy routing —
  authority/escalation-class failures notify the owner immediately,
  connector/resource-class failures batch into the owner digest; authenticated
  API bad requests surface directly without duplicate owner notification; this
  brief OWNS the minimal digest substrate: encrypted stable detail references,
  deterministic lossless UTF-8 pagination, detail-specific receipts, and
  fail-closed unavailable-detail audit. External send remains delivery-unknown
  across a crash before receipt commit and may retry. Presentation format is
  AD-082 *leaning*, refined by implement-personality-seed. Per-connector
  success/failure counters remain kernel tables (no external metrics stack).
- **Done when:** the fire-and-forget audit-append pattern is gone; an injected
  audit-append failure fails the action under test; an injected notify failure
  produces the truthful event sequence plus an encrypted-reference dead-letter
  entry; a bad request produces no duplicate owner notification; a batched
  connector failure is losslessly owner-retrievable across bounded pages; and
  crash-before-receipt recovery remains fenced and truthfully delivery-unknown.

#### implement-day2-operations

- **Canon:** AD-139; AD-144 (first-run posture, documented at current depth).
- **Requires:** implement-failure-surfacing-contract.
- **Scope:** versioned schema migrations (`PRAGMA user_version`) with a
  documented downgrade path; backup/restore treats SQLite DB + artifact blobs
  + key material as ONE consistent snapshot set with a documented restore
  drill; disk-full degrades loudly (actions fail per AD-138, never run
  unrecorded); clock-regression detection at boot; first-run bootstrap
  sequence documented as-is with its failure messages (AD-144); simultaneous
  owner devices are one identity — same-conversation races serialized per
  AD-102's (*leaning*) one-message-per-conversation rule, as bound by settled
  AD-144.
- **Out:** runtime blue/green kernel trial (deferred); richer onboarding UX
  (deferred until a second deployment exists).
- **Done when:** migration up/down exercised under test; disk-full fails the
  action loudly under test; clock regression at boot is detected under test;
  the restore drill and the first-run bootstrap sequence (with failure
  messages) are documented.

#### implement-connector-reality

- **Canon:** AD-141; AD-103.
- **Requires:** refactor-kernel-registries; implement-failure-surfacing-contract.
- **Scope:** per-connector rate-limit buckets with backoff; token
  refresh-before-expiry; webhook signature verification, idempotency keys,
  bounded replay windows (a spoofed or replayed webhook is an attack path,
  handled structurally); per-connector circuit breaker (Closed/Open/HalfOpen)
  in the gate path with a `connector_unavailable` audit event distinct from
  policy denial; per-call timeouts on connector calls.
- **Out:** bulkhead resource pools (deferred per AD-103's single-owner
  rationale).
- **Done when:** a replayed or unsigned webhook is rejected under test; an
  Open breaker blocks the effect with the distinct audit event.

#### implement-spend-kill-switch

- **Canon:** AD-143.
- **Requires:** implement-failure-surfacing-contract.
- **Scope:** global per-day spend cap across all model calls and connector
  usage, sitting above per-task grant budgets; breach pauses the proactive and
  headless lanes (binding on those lanes as they land) and notifies the owner
  on the immediate lane.
- **Done when:** a simulated breach blocks further grant composition/dispatch
  on any non-immediate lane (lane simulated in the test) and emits an
  immediate owner notification under test.

#### implement-secret-intake

- **Canon:** D-014 (env-var bootstrap is an explicitly documented deferral).
- **Requires:** none.
- **Scope:** broker/vault secret intake and rotation replacing env-var-only
  secrets; secrets never visible to the shell (D-005/D-010 posture preserved).
- **Done when:** a secret can be introduced and rotated without a process
  restart and without shell visibility.

### Overlay & key model

#### implement-overlay-model

- **Canon:** AD-070; AD-071; AD-023.
- **Requires:** none.
- **Scope:** base/overlay namespacing on artifacts — base upstream-owned and
  versioned, overlay user-owned and update-surviving; update compat pass with
  one-tap re-confirmation of orphaned learned artifacts (never silently
  broken, never silently kept); provenance links on every learned artifact to
  the exchange that produced it; upstream nomination of generalized patterns
  is explicit opt-in through normal review.
- **Done when:** an update that orphans a learned artifact surfaces
  re-confirmation under test; every learned artifact carries provenance.

#### implement-counterparty-key-model

- **Canon:** AD-140.
- **Requires:** implement-identity-store-and-principal;
  implement-overlay-model (provenance links drive invalidation).
- **Scope:** per-counterparty payload keys replacing the single global
  artifact-store key; crypto-erase = key deletion — plaintext unrecoverable
  while the hash chain keeps tamper evidence; derived overlay artifacts
  invalidated via their provenance links.
- **Done when:** erasing a counterparty renders its payloads unrecoverable,
  chain verification still passes, and derived artifacts are invalidated
  under test.

#### implement-overlay-export-restore

- **Canon:** AD-150.
- **Requires:** implement-counterparty-key-model (**HARD** — from debate: the
  single-key design must not be baked into the export format);
  implement-day2-operations (snapshot set); implement-overlay-model.
- **Scope:** owner-only gated export/restore actions; bundle = SQLite DB +
  artifact blobs + key material as one atomic snapshot; documented restore
  drill; base-version migration runs the AD-070/071 compat pass.
- **Done when:** an export→restore round-trip onto a newer base version passes
  the compat pass under test.

### Authority growth

#### implement-plan-digest-approval

- **Canon:** AD-011; D-011.
- **Requires:** none.
- **Scope:** generalize digest-bound approval from email-body-shaped to
  plan-shaped: a clarifying question carries the plan digest (all effectful
  steps including data-handling steps); the owner's "yes" approves exactly
  that digest; kills the deferential double-ask structurally.
- **Done when:** a plan approval binds to the digest of the full step list; a
  mutated plan after approval is refused (WYSIWYS parity with D-011/D-045).

#### implement-overlay-eval-gate

- **Canon:** AD-142; AD-110; AD-111 (*leaning* — judge protocol details).
- **Requires:** define-lineage-and-eval-store.
- **Scope:** every authority-bearing proposal (standing rules foremost) passes
  offline replay against captured owner history plus an adversarial risk-judge
  pass BEFORE the owner's approval tap; verdicts land in the eval store;
  evidence attaches to the proposal so one-loop confirmation is informed;
  adversarial passes at promotion points only, never per-use;
  quiet-activating preference suggestions keep the lighter bar (AD-001).
- **Done when:** an authority-bearing proposal structurally cannot reach the
  approval surface without attached replay + judge evidence.

#### implement-standing-rules

- **Canon:** AD-010; AD-106; AD-013; AD-012 (*leaning* — dark-window
  defaults).
- **Requires:** implement-overlay-eval-gate; implement-durable-workflow-replay
  (scoped edge: only the dark-window timer requirement rides on it).
- **Scope:** standing-rule artifact class — versioned, revocable, expiry,
  drift triggers; consultation at gate/compose time on live actions; quota
  (volume) vs rate (velocity) sliding-window counters checked at GATE time in
  the same transaction as the decision (failed effects don't consume budget;
  atomic upsert per the D-050 precedent); remaining budget returned in the
  gate response (feeds AD-013 calibration); dark-window conditional grants as
  standing rule + kernel timer, highest-scrutiny audit case.
- **Invariant:** standing rules are composition INPUTS; the task grant remains
  the only live authority object (D-007).
- **Done when:** repeated approval → proposal → eval-gate evidence → one-tap
  confirm → a matching action passes gate without approval, within budget,
  with the decrement visible in the gate response — all under test.

#### implement-model-swap-ceremony

- **Canon:** AD-152.
- **Requires:** implement-overlay-eval-gate.
- **Scope:** any base/matcher/miner model swap is a PROPOSAL carrying
  golden-set replay evidence (the spec defines the golden-set format and
  pass/fail criteria) through the AD-142 gate, digest-approved by the owner;
  no silent swaps.
- **Done when:** a swap without attached evidence is structurally impossible;
  the golden-set format is a spec requirement.

#### implement-disclosure-policy

- **Canon:** AD-002; AD-146 (disclosure half).
- **Requires:** implement-identity-store-and-principal;
  implement-standing-rules (answers become standing rules with carve-outs);
  implement-briefcase-packing (classified briefcase items feed the egress
  checks); implement-escalation-and-refusal (the owner-question escalation
  surface).
- **Scope:** `DisclosurePolicy` artifact keyed (relationship ×
  disclosure-class); outbound queries built from private context are effects;
  provenance tracking from classified briefcase items enables deterministic
  egress checks (no LLM judging sensitivity); query generalization before
  egress; policies acquired lazily — a counterparty hits a deterministic gate
  block, the owner answers one honest question (AD-133 surface), the answer
  becomes a standing rule with carve-outs.
- **Done when:** a disclosure-class egress without a covering policy blocks
  deterministically and produces the owner-question escalation under test.

#### implement-egress-classes

- **Canon:** AD-060.
- **Requires:** refactor-kernel-registries.
- **Scope:** egress endpoints typed and policy-rated in the connector registry
  (no-log search API ≠ forum browse ≠ web-form POST); packs reference egress
  classes.
- **Done when:** a pack granted search-class egress cannot submit a web form
  under test.

### Delegation & containment

#### implement-briefcase-packing

- **Canon:** AD-021; AD-031; AD-032; AD-121.
- **Requires:** implement-identity-store-and-principal.
- **Scope:** kernel packs every task's context deterministically from task
  shape (route × workflow × counterparty): grant + relevant preferences +
  relevant skills + counterparty slice; briefcase as kernel-owned blackboard
  with visibility classes (kernel-bound / worker-scratch / returned-output)
  and a per-worker visibility record; depth = f(relationship tier × task
  class); worker top-up requests are gate-visible.
- **Invariant:** the kernel decides packing; the master agent only
  proposes/requests (AD-032 — confused-deputy defense).
- **Done when:** identical task shape yields an identical pack (determinism
  test); visibility classes are enforced under test.

#### implement-worker-runtime

- **Canon:** AD-030; AD-033; AD-035; AD-101 (*leaning* — runtime chain
  minting).
- **Requires:** define-grant-chain-and-modes; implement-briefcase-packing;
  implement-event-bus-subscriptions.
- **Scope:** runtime sub-grant minting as caveat chains, gate-verifiable
  offline; master agent = interpret/commission/relay only — work runs in
  separate task-granted shells, results return as events; reply chokepoint —
  worker→master crossings are schema-checked structured results, free-text
  fields stay wrapped as untrusted.
- **Done when:** chain verification rejects widening under test; a
  commissioned worker's result returns as a structured event with free text
  wrapped.

#### implement-worker-supervision

- **Canon:** AD-100; AD-102 (*leaning* — identity addressing).
- **Requires:** implement-worker-runtime.
- **Scope:** OTP-style supervision with authority reset: a restarted worker
  NEVER inherits the dead worker's grant, `worker_failed` is a structured
  event, continuation requires re-composition through the normal pipeline;
  restart-intensity caps per connector; identity-tuple worker addressing with
  one message at a time per conversation.
- **Done when:** worker crash → `worker_failed` → continuation requires
  re-composition, all under test; restart caps hold under a flaky-connector
  test.

#### implement-escalation-and-refusal

- **Canon:** AD-133; AD-151; AD-148.
- **Requires:** implement-identity-store-and-principal. (Worker escalation
  events from implement-worker-runtime are its main producer; the routing
  machinery itself does not depend on it.)
- **Scope:** deterministic kernel escalation routing (route by task; workers
  talk to the owner only when escalating); refusals stay enum reason codes —
  workers receive outcomes, never policy text; ONE canonical policy-free
  deferral plus deterministic escalation; presentation phrasing learnable, the
  no-leak invariant kernel; optional `thread_id` on EventEnvelope and
  TaskGrant with kernel-owned thread↔grant binding, DORMANT until a
  thread-capable channel ships (debate correction: Telegram topics are
  group-only; owner control stays private-chat).
- **Done when:** a gate denial facing a counterparty surfaces only the
  canonical deferral plus an escalation event; no policy text crosses the
  chokepoint under test.

### Skills & workflows

#### implement-workflow-state-machines

- **Canon:** AD-044; AD-046/AD-122 (*leaning* — static per-step tier map;
  LOD-trader knapsack deferred).
- **Requires:** implement-durable-workflow-replay.
- **Scope:** workflows as declarative state machines (YAML,
  mermaid-renderable): states, transitions, agentic vs deterministic steps,
  escalation points, approval semantics mapped on workflow states; per-step
  reasoning-tier declaration with a static tier map consumed by the gateway.
- **Done when:** approval semantics are enforced at the declared states under
  test; a step's declared tier is respected in gateway calls.

#### implement-skill-artifact-class

- **Canon:** AD-040; AD-041; AD-042; AD-043 (*leaning* — external-import
  pipeline, deferred until the first external import is wanted).
- **Requires:** refactor-kernel-registries; implement-overlay-eval-gate
  (promotion review per AD-110); implement-failure-surfacing-contract
  (digest surface for the containment test).
- **Scope:** skills as a versioned artifact class shaping competence only;
  install/update ceremony separate from `artifact.propose`, proportionate to
  provenance (shipped-seed and user-installed trusted at install;
  miner-distilled one-tap with provenance + diff); use is silent — the kernel
  injects installed skills by task shape; selection = deterministic index +
  semantic-matcher fallback selecting ONLY from the approved shelf (matcher
  can inject, never install); skill visibility scoped per agent/pack.
- **First task (REQUIRED):** a formal D-0XX revisit entry for D-048 grounded
  in the gate-containment guarantee, BEFORE any runtime skill machinery ships
  (AD-040's stated basis).
- **Done when:** the matcher cannot install; a poisoned-skill exfiltration
  attempt dies at the gate and surfaces in the digest (containment test).

#### implement-authority-equivalence-matcher

- **Canon:** AD-147; AD-124.
- **Requires:** refactor-kernel-registries (**HARD** — validated ActionId
  registry); implement-workflow-state-machines;
  implement-skill-artifact-class.
- **Scope:** the kernel — never the shell — computes equivalence classes
  deterministically from artifacts' DECLARED action lists; class identity =
  identical composed (allowed_actions, approval_required_actions,
  denied_actions, output_channels, limits); the semantic matcher picks freely
  WITHIN one class and never across; cross-class ambiguity resolves by
  deterministic rule or escalation; classes are auditable and testable, never
  LLM-derived; matcher model swaps ride implement-model-swap-ceremony.
- **Invariant:** refines D-008, never repeals it — the LLM still never
  resolves route conflicts or constructs grants; within-class members are
  authority-identical by construction.
- **Done when:** property test — any within-class pick composes an identical
  grant; a cross-class pick is structurally impossible.

#### implement-seed-workflows

- **Canon:** AD-153.
- **Requires:** implement-workflow-state-machines; implement-overlay-model.
- **Scope:** the minimal seed set — owner-control conversation,
  email-draft-with-approval, research-and-brief, customer-service intake
  template — shipped as overlay artifacts, never kernel fixtures; everything
  else arrives via miner proposals.
- **Done when:** seeds live in the overlay namespace, learnable and
  replaceable per AD-070.

### Reflection & product surface

#### implement-reflection-miner

- **Canon:** AD-149; AD-050 (scheduled tier); AD-053; AD-054; AD-022.
- **Requires:** implement-briefcase-packing; implement-worker-runtime;
  implement-overlay-eval-gate; implement-overlay-model (learned-artifact
  provenance and the consolidation target class).
- **Scope:** the miner runs under an ORDINARY task grant — classification
  ceiling, empty output_channels, model-call/artifact limits, model calls
  through gateway and gate; briefcase = scoped audit-trail slice; outputs are
  proposable artifacts through the normal lifecycle, never direct mutation of
  kernel state or standing rules; output classes: corrections-with-reasons,
  repeated approvals (standing-rule candidates), stated preferences;
  positive-steering discipline — corrections REWRITE instructions, never
  append prohibitions; negative constraints become eval probes; periodic
  consolidation/autophagy pass merges/prunes learned artifacts.
- **Invariant:** never a privileged background daemon — that would be a covert
  channel around gate/budget (AD-149).
- **Done when:** the miner cannot write kernel state; its proposals carry
  provenance; a correction yields a rewrite proposal, not a prohibition
  append, under a probe test.

#### implement-nerve-subscribers

- **Canon:** AD-130 (*leaning*); AD-132 (*leaning*); AD-051; AD-112; AD-052;
  AD-034 (screener).
- **Requires:** implement-event-bus-subscriptions; implement-egress-classes
  (AD-149 names the AD-060 registry and the AD-105 bus as N-nerve
  prerequisites).
- **Scope:** nerve declaration schema — subscription filter × measure ×
  speak-threshold × budget × model tier × scope (≤ advisee); declared types:
  advisor, injector, screener (inbound manipulation tagging per AD-034),
  miner, meta-cognition; advisor = legibility checker producing structured
  objections (concern class + cited clause + suggested rewrite), never
  "better answers"; advisor data access never exceeds its advisee's;
  cross-scope hints cross as structured gate-visible messages; proactivity as
  a budgeted lane — hard budgets, provenance required, user reaction mined,
  ignored-interjection decay retires noisy classes.
- **Done when:** a nerve declared with broader data scope than its advisee is
  unregistrable; interjections carry structure and consume budget under test.

#### implement-persona-binding-and-headless-lanes

- **Canon:** AD-136; AD-134.
- **Requires:** implement-identity-store-and-principal;
  implement-connector-reality (webhook authenticity for hook lanes).
- **Scope:** WHICH persona fronts a conversation resolves deterministically at
  route time from (connector/number × sender identity × relationship), never
  by the agent; personas are overlay artifacts, their BINDING is kernel
  machinery; hooks (webhooks) are ordinary event sources through the full
  pipeline — verified, identified, routed, granted, gated — with no owner
  conversation when composed authority requires no approval; surfacing volume
  is a learned overlay preference.
- **Done when:** the owner reaching any bound number gets the owner-facing
  persona and a counterparty structurally cannot (route test); a verified
  hook completes a no-approval pipeline flow end-to-end with zero
  conversation, surfaced only in the owner digest.

#### implement-personality-seed

- **Canon:** AD-080; AD-081; AD-083; AD-082 (*leaning* — digest format ships
  as a learnable default per AD-135).
- **Requires:** implement-overlay-model; implement-overlay-eval-gate
  (anti-pattern probes live in the eval harness).
- **Scope:** the eight-element Donna×Leo seed as pre-populated learnable
  overlay artifacts (never kernel-baked); anti-patterns — including the
  AD-083 additions (faked intimacy, info-dump without synthesis,
  self-promotional visibility) — as testable eval probes; digest/brief
  default: ≤3 priority items, decisions-needed → FYI → handled, one line
  each, detail behind the fold.
- **Done when:** seed artifacts live in the overlay (learnable, divergent);
  every AD-081/AD-083 anti-pattern has an eval probe.

## Reconciliation of the previous "later changes" list

The dispositions below are the authoritative mapping; D-051 records the
rationale.

- `implement-secret-intake` — carried forward; see its brief under Failure
  surfacing & operations.
- `implement-route-artifact-lifecycle` — SUBSUMED by
  `implement-artifact-lifecycle-slice` (archived 2026-07-03): propose →
  approve → activate covers route artifacts.
- `implement-agent-manifest-registry` — proposal/activation covered by
  `implement-artifact-lifecycle-slice`; runtime registry consumption folds
  into `refactor-kernel-registries`.
- `implement-capability-pack-registry` — same disposition as the agent
  manifest registry; egress-class references land in
  `implement-egress-classes`.
- `implement-memory-policy` — superseded by `implement-overlay-model` +
  `implement-briefcase-packing` + `implement-disclosure-policy` (the AD-020
  two-kinds split distributes the old placeholder's intent).
- `implement-deployment-reference` — superseded by
  `implement-day2-operations` (AD-139) plus the AD-144 bootstrap
  documentation; the docs site carries the deployment reference.
