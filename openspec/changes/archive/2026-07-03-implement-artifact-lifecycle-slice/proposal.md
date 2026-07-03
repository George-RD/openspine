# Proposal: Implement artifact lifecycle slice

## Summary

Implement the PRD's dynamic-growth thesis minimally: an agent proposes a declarative artifact over `artifact.propose`; the kernel validates it, persists it, and asks the owner; a digest-bound Telegram approval activates it into the live registry and an on-disk overlay.

## What Changes

`artifact.propose` moves from a stub to a real dispatch: it schema-validates the proposed YAML per kind, budgets it as a shell-initiated artifact put, persists it as `proposed`, and mints a digest-bound `artifact.activate` approval request. The existing digest-bound approval callback (D-039/D-040/D-041/D-044) gains a second branch: on `artifact.activate`, the kernel re-parses the exact approved YAML, flips its `lifecycle_state` to `active`, writes it into the `data/artifacts.d` overlay, and inserts it into the live, now-lockable `ArtifactRegistry` so it participates in routing/composition immediately.

## Why

Every other slice so far composes authority from artifacts that only ever change by editing files on disk and restarting the kernel. The PRD's "make dynamic behaviour easy, dynamic authority hard" thesis requires a real runtime path for authority-affecting artifacts to grow — gated by the same owner-approval machinery every other effectful action already goes through, never by anything the shell or an agent can trigger unilaterally.

## Affected layer

Both OpenSpine core (registry, store, gate-adjacent dispatch) and Lyra product (the `/propose` command surface).

## Authority sensitivity

Authority-sensitive. This is the first runtime path that can add a new `allowed_actions`/`approval_required` entry, route, or policy to the live system without a kernel restart.

## Goals

- Validate proposed artifacts against their schema before persisting anything.
- Require an exact-payload, digest-bound owner approval before any proposed artifact can activate (D-048; reuses D-011/D-039-D-044 machinery).
- Enforce the PRD lifecycle chain (`proposed → validated → review_required → approved → active`), rejecting illegal transitions.
- Enforce id+version uniqueness across fixtures, the on-disk overlay, and pending proposals.
- Persist activated artifacts to an overlay directory that survives a kernel restart.
- Only `active` artifacts participate in authority composition — unchanged from today.

## Non-goals

- Do not make prompt templates proposable (D-048 — a template changes the model's instruction surface; a dedicated future change, not this slice).
- Do not add widening-detection heuristics that let some proposals skip owner approval — every proposal requires the same explicit approval (D-048).
- Do not implement the quarantine/retire runtime transitions — the schema already models them; no runtime path yet.
- Do not wire the PRD's per-kind activation ids (`route.activate`, `workflow.activate`, `capability_pack.change`, `policy.change_proposal`) — they remain candidate, unwired entries (D-048, mirrors D-034).

## Decision-log check

This change preserves the accepted decisions in `.raw/openspine-decision-log.md`, and adds D-048 for the new choices this slice makes (canonical activation action id, uniform approval, template exclusion).

If implementation reveals a need to weaken, reverse, or materially refine an accepted decision, update the decision log before completing the change.
