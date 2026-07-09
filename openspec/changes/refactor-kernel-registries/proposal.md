# Proposal: Refactor kernel registries

## Summary

Convert the kernel's four hardcoded extension points — connectors, allowed-action dispatch, proposable artifact kinds, and the (currently nonexistent) canonical ActionId set — from match-arms and concrete struct fields into registries, one-to-one and behavior-preserving, plus the one new behavior the canon requires: unknown ActionIds fail fast with a structured error at authority composition and at gate.

## What Changes

- A `ConnectorRegistry` replaces `AppState.telegram` / `AppState.gmail` as the single registration point for connectors, preserving Gmail's load-bearing optionality (the "not configured" graceful-degradation paths).
- An `ActionHandlerRegistry` replaces `dispatch_allowed_action`'s string match; the honest-stub fall-through for authorized-but-unimplemented action ids is preserved, and approval-gated ids (`email.create_draft`, `artifact.activate`) remain undispatchable directly. The post-approval resolution match gains the same registry treatment with its everything-else-is-a-draft default kept.
- An artifact-kind table becomes the single source of truth for the five proposable kinds (name, overlay subdirectory, parse, duplicate check), replacing the three drift-prone parallel matches (`PROPOSABLE_KINDS` guard, `parse_proposal`, the propose dup-check). Prompt templates remain non-proposable (D-048).
- A curated, canonical `ActionCatalog` of known ActionIds is introduced (none exists in production today). `compose_authority` rejects a candidate id absent from the catalog with a structured error; `gate()` denies an unknown id with a structured reason distinct from `NotGranted`.

## Why

Kernel-readiness item 1 (`.raw/openspine-agentos-design-log.md`): "Registries over match-arms: Connector trait+registry, ActionHandler registry, ProposableArtifact kind registry; fail-fast unknown ActionIds." It is the named prerequisite of AD-147 (authority equivalence matcher) and the first change of the agent-OS sequence: every later change that adds a connector (AD-060, AD-103), an action, or an artifact kind builds on registration instead of scattering match-arm edits across the kernel.

## Affected layer

OpenSpine core (kernel, authority, gate, schemas). No Lyra product surface changes; existing flows are unchanged.

## Authority sensitivity

Authority-adjacent. No authority rule changes, but composition and gate gain a stricter failure mode: an ActionId outside the canonical catalog can no longer silently ride through composition into a grant — it fails composition; at gate it is denied with a structured `unknown` reason. Known-but-unimplemented ids (e.g. `route.activate`, `memory.read:owner_preferences_limited`) remain valid, grantable, and stub-dispatched exactly as today.

## Goals

- Adding a connector, action handler, or proposable artifact kind is a registration at one declared point, not a match-arm edit hunted across files.
- One-to-one behavior preservation: every existing test passes unchanged in meaning (mechanical updates to constructor call sites only).
- Unknown ActionId ⇒ structured error at composition and structured denial at gate, each with a test.
- The canonical catalog includes every action id referenced by the shipped fixtures, including intentionally unwired PRD ids (`route.activate`, `workflow.activate`, `capability_pack.change`, `policy.change_proposal`, `connector.enable`).

## Non-goals

- No circuit breakers, health states, or egress typing in the connector registry (AD-103 / AD-060 — later changes).
- No runtime registration of new actions/connectors/kinds — registries are compiled-in registration points; runtime growth stays behind the artifact-lifecycle approval path.
- No pipeline-driver restructuring (that is `refactor-pipeline-driver`).
- No new proposable kinds; templates stay excluded (D-048).

## Decision-log check

This change preserves the accepted decisions in `.raw/openspine-decision-log.md` (notably D-004, D-006, D-007, D-033, D-048) and adds D-053 for the new choices it makes: the curated canonical ActionCatalog as a kernel const, unknown-at-composition as a hard error, and unknown-at-gate as a structured denial distinct from `NotGranted`.

If implementation reveals a need to weaken, reverse, or materially refine an accepted decision, update the decision log before completing the change.
