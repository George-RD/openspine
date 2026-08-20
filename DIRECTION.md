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
- The openspec ceremony (propose → apply → archive) is **retired for new
  work**. Do not open new changes under `openspec/changes/`.
- In-flight openspec changes at ruling time: **none** (`openspec/changes/`
  held only `archive/`). The Effect Truth track (#173, #174) had not entered
  openspec and proceeds under the new workflow. Nothing to migrate.
- The archived spec tree (`openspec/specs/`, `openspec/changes/archive/`)
  remains the historical record; `scripts/check.sh` continues to validate it.
  Retiring that tooling is housekeeping for after the new workflow proves
  out, not part of any current map.
- **Wayfinder handoff shape**: a design (grilling) ticket resolves when its
  decision is recorded *and* a to-spec spec issue exists on the tracker,
  linked from the resolution comment. `to-tickets` and `implement` run
  off-map from that spec. Fold tickets resolve by comments on the issues they
  fold into; filing tickets resolve by the filed issue. Implementation order
  must be derivable from the tracker alone.

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
- If no map decision cites the Outside-framework story, foreign-host mode is
  recorded dormant here and nothing that only it needs is paid for.
