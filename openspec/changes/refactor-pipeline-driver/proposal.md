# Proposal: Refactor pipeline driver

## Summary

Re-express the kernel's two hardcoded event flows — the Telegram owner-control flow and the `/draft` selected-thread email preview flow — as data interpreted by a single typed pipeline driver. The nine-stage sequence (event→verify→identify→route→compose→grant→run→gate→audit) becomes a declared, typed stage sequence; per-flow variation (channel trust, lane classification, envelope construction, extra verification, selection-token minting, target route) becomes a lane specification record. Behavior-preserving: every existing test passes unchanged in meaning, and audit event sequences are unchanged.

## What Changes

- A typed `PipelineStage` sequence declares the nine stages in one place; the driver executes the synchronous prefix (event→verify→identify→route→compose→grant→run) in that fixed order.
- A `LaneSpec` data record captures everything that differs between the two current flows. `handle_owner_update` and `handle_thread_selection` — today two near-duplicate ~230-line driver functions (`pipeline/mod.rs:207-478`, `pipeline/selection.rs:42-390`) — collapse into one driver plus two lane specifications: the owner-control lane and the selected-thread email preview lane.
- `gate()` and audit are NOT relocated. Gate remains a distributed runtime stage invoked at the shell's HTTP dispatch surface (`api/actions.rs`, `api/generate.rs`) and re-invoked at the approval callback (`pipeline/approval.rs`), per AD-120 (the shell sends intents; the kernel computes outcomes). Audit remains the cross-cutting per-stage `append_audit` weave with unchanged event names and order.
- `handle_draft_approval_callback` stays as the gate-stage runtime path (registered post-approval resolution, unchanged) — it is not a third lane.
- The containment guard (`refuses_external_communication_without_containment`) becomes lane-driven data: load-bearing for the external-communication lane, a structural no-op for the owner-control lane, exactly as today.

## Why

Kernel-readiness item 2 (`.raw/openspine-agentos-design-log.md:252`): "Pipeline driver: typed stage sequence, lanes as data (current flows = first two lanes)." AD-134 validates that the pipeline is event-shaped, not chat-shaped: future lanes (webhook-triggered headless workflows, AD-141 hook lanes) must be additions of lane data, not third and fourth copies of a hand-rolled driver function. Today every new flow means duplicating the stage sequence inline and hoping the copies do not drift; the two existing copies already diverge only where the canon says lanes should differ.

## Affected layer

OpenSpine core (kernel `pipeline` module). No authority rule changes, no schema changes to grants or events, no Lyra product surface changes.

## Authority sensitivity

Behavior-preserving refactor; not authority-sensitive. The composition, grant, gate, and audit semantics are byte-for-byte the semantics shipped today. The one structural tightening: the stage order and per-lane behavior are declared data, so a future lane cannot accidentally skip verification or the containment guard by forgetting a line in a copied driver.

## Goals

- Adding a lane is writing a lane specification, not writing (and reviewing) a fourth driver function.
- One driver owns the stage order; the order is declared once and cannot drift between lanes.
- The two current flows are expressed as the first two lanes with zero behavior change: all existing pipeline, API, authority, and gate tests pass unchanged in meaning; audit event names and ordering are unchanged.
- Gate's placement at the effect boundary (runtime, not driver-prefix) is made explicit and structural.

## Non-goals

- No new lanes, event sources, or webhook intake (AD-134/AD-141 — later changes).
- No changes to gate(), authority composition, or grant semantics.
- No runtime-proposable lanes: lane specifications are compiled-in kernel data, not artifacts; runtime growth stays behind the artifact-lifecycle approval path.
- No workflow state machines (AD-044 — `implement-workflow-state-machines`).
- No per-stage latency budgets (OQ-8 — explicitly open).
- No identity-store work: the identify stage keeps calling `resolve_owner_identity` as today (`implement-identity-store-and-principal` replaces its internals later).

## Decision-log check

This change preserves the accepted decisions in `.raw/openspine-decision-log.md` (notably D-004, D-005, D-007, D-008) and adds D-054 for the choices canon leaves open: stages as a typed compiled-in sequence the driver executes; lanes as compiled-in data records with a single-stage hook contract, never runtime artifacts; gate modeled as a distributed runtime stage outside the driver's synchronous prefix; lanes forbidden from reordering or omitting stages; the audited event envelope (`event.received`) emitted only after verification succeeds, preserving today's preflight-failure audit surface.

If implementation reveals a need to weaken, reverse, or materially refine an accepted decision, update the decision log before completing the change.
