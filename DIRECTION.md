# DIRECTION

Standing rulings. Sessions consult this before assuming process or reopening
a decision. Vocabulary: [CONTEXT.md](CONTEXT.md); user stories:
[STORIES.md](STORIES.md). Requirement content stays in the `.raw/` canon; on
*process*, this file is newest and wins.

## Delivery vehicle (ruled 2026-08-20)

- The Pocock skills workflow is this repo's native delivery vehicle:
  **wayfinder** (decision maps) → **grilling/domain-modeling** (design) →
  **to-spec** (spec issue on the tracker, labelled `ready-for-agent`) →
  **to-tickets** (tracer-bullet implementation issues with native blocking
  edges) → **implement**.
- The OpenSpec ceremony (propose → apply → archive) is **retired**. The tracker
  workflow is the sole delivery path; never create `openspec/changes/` entries.
- In-flight openspec changes at ruling time: **none** (`openspec/changes/`
  held only `archive/`). The Effect Truth track (#173, #174) had not entered
  openspec and proceeds under the new workflow. Nothing to migrate.
- `openspec/specs/` remains the canonical historical spec snapshot and
  `scripts/check.sh` strictly validates it. `openspec/changes/archive/` remains
  static change history. The retired ceremony skills and commands were removed
  on 2026-09-01; OpenSpec is no longer a delivery tool.
- **Wayfinder handoff shape**: a design (grilling) ticket resolves when its
  decision is recorded *and* a to-spec spec issue exists on the tracker,
  linked from the resolution comment. `to-tickets` and `implement` run
  off-map from that spec. Fold tickets resolve by comments on the issues they
  fold into; filing tickets resolve by the filed issue. Implementation order
  must be derivable from the tracker alone.

## Delivery mechanics (ruled 2026-09-01)

- Code reaches `main` through pull requests, never direct pushes.
- An implementation issue may use a bounded stack of 2–4 complete, tested PR
  layers only when the layers are genuinely dependent. Independent work stays
  unstacked.
- `/code-review` covers every layer. One independent adversarial review covers
  the exact cumulative stack head; trust-boundary changes also get a specialist
  review. Changed diffs or trees invalidate the corresponding evidence.
- [`docs/agents/delivery.md`](docs/agents/delivery.md) is the operational
  workflow for branching, gates, review evidence, stack synchronization, and
  merge.

## Decided — not to reopen (2026-08-20)

- Typed effect disposition and disposition-driven settlement (#173/#174)
  precede everything that writes through them.
- Typed owner identity **design** precedes the tenancy assessment; tenancy
  **implementation** waits for the fit review.
- Bell v1 deploys instance-per-tenant, so kernel tenancy is consolidation,
  not launch-blocking.
- The WhatsApp connector is built during Bell's integration spike against
  real requirements, never before.
- Bell's worker starts in-tree; it splits out once the kernel contract
  survives the spike.
- The Immune system promise gates Bell serving real customers. Charted as a
  design lane: capability-derived tool catalogs, disclosure gating on
  external egress, provenance labels with typed identity. Designs, not
  implementation.
- Orchestration lives in workers, permanently; workflows live in the kernel;
  products are packages, never bespoke kernel code.

## Decision test (normative)

Land a candidate that serves two or more users (STORIES.md); fold it where an
existing ticket carries those users; reject what serves one imagined story.
Correctness fixes and the five promises (CONTEXT.md) need no story.

## Facts

- The 41 MiB graphify history rewrite is decided and owner-executed. A fact,
  not a ticket.
- The Bell roadmap is canon-as-context: its priorities hold (Effect Truth
  first; the fit review gates tenancy implementation and P3); decisions may
  add shape to its tickets via comments and links.
- **Foreign-host mode: DORMANT** (ruled 2026-08-20, map #182 closing census —
  ticket #194). No decision on the deepening map cited the Outside-framework
  story: every landed design cites Lyra, Bell, the Unattended workhorse, or
  the Auditor. Nothing whose only consumer is foreign-host mode may be built
  or preserved; session-surface effect governance returns only via a redrawn
  destination, as a fresh effort.
