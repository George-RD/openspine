# Graph Report - openspine  (2026-07-02)

## Corpus Check
- 5 files · ~15,113 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 166 nodes · 528 edges · 9 communities detected
- Extraction: 91% EXTRACTED · 9% INFERRED · 0% AMBIGUOUS · INFERRED: 45 edges (avg confidence: 0.85)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- [[_COMMUNITY_Community 0|Community 0]]
- [[_COMMUNITY_Community 1|Community 1]]
- [[_COMMUNITY_Community 2|Community 2]]
- [[_COMMUNITY_Community 3|Community 3]]
- [[_COMMUNITY_Community 4|Community 4]]
- [[_COMMUNITY_Community 5|Community 5]]
- [[_COMMUNITY_Community 6|Community 6]]
- [[_COMMUNITY_Community 7|Community 7]]
- [[_COMMUNITY_Community 8|Community 8]]

## God Nodes (most connected - your core abstractions)
1. `gate() — Action Mediation Function` - 48 edges
2. `Task Grant (Live Authority Object)` - 41 edges
3. `Audit Event` - 19 edges
4. `Approval Record Schema` - 19 edges
5. `Selected-Thread Selection Token` - 17 edges
6. `Deny-by-Default Authority Policy` - 16 edges
7. `Design: Core Runtime Schemas` - 16 edges
8. `Event Envelope` - 14 edges
9. `Change: Define Dev Process — Proposal` - 13 edges
10. `Gate Decision Type` - 13 edges

## Surprising Connections (you probably didn't know these)
- `Repo Index (Markdown)` --semantically_similar_to--> `Repo Index (Plain Text)`  [INFERRED] [semantically similar]
  repo-index.md → repo-index.txt
- `Selected-Thread Selection Token` --semantically_similar_to--> `Approval Record Schema`  [INFERRED] [semantically similar]
  openspine-full-openspec-conflux-bundle/openspec/specs/core-runtime-schemas/spec.md → openspine-remaining-openspec-bundle/openspec/changes/define-core-runtime-schemas/design.md
- `Selected-Thread Selection Token` --semantically_similar_to--> `Route Artifact Schema`  [INFERRED] [semantically similar]
  openspine-full-openspec-conflux-bundle/openspec/specs/core-runtime-schemas/spec.md → openspine-remaining-openspec-bundle/openspec/changes/define-core-runtime-schemas/design.md
- `Deny-by-Default Authority Policy` --semantically_similar_to--> `Authority-Sensitive Change Marking`  [INFERRED] [semantically similar]
  openspine-full-openspec-conflux-bundle/openspec/specs/authority-composition/spec.md → openspec/changes/define-openspine-development-process/design.md
- `Skills Index (Markdown)` --semantically_similar_to--> `Skills Index (Plain Text)`  [INFERRED] [semantically similar]
  skills-index.md → skills-index.txt

## Hyperedges (group relationships)
- **Authority Enforcement Pipeline** — event_envelope, route_resolution, authority_composition, task_grant, gate_function [INFERRED 0.90]
- **Bounded Email Workflow Security Flow** — selection_token, digest_bound_approval, spec_email_preview, spec_digest_approval, model_gateway [INFERRED 0.82]
- **OpenSpec Development Change Lifecycle** — skill_openspec_explore, skill_openspec_propose, skill_openspec_apply_change, skill_openspec_archive_change [EXTRACTED 1.00]
- **Gate Enforcement Flow: action request evaluated against task grant to produce gate decision with audit** — gate_function, action_request, task_grant, gate_decision, audit_metadata [EXTRACTED 0.95]
- **Authority Composition Pipeline: event + identity + route + policy compose into task grant via deny-by-default** — authority_composer, deny_by_default, event_envelope, route_artifact, task_grant [EXTRACTED 0.92]
- **Selected-Thread Email Preview Workflow: selection token gates read, model gateway processes, preview output produced** — selection_token, email_thread_selected_event, email_reply_drafter, model_gateway, gate_function [EXTRACTED 0.90]
- **Gate Enforcement Mechanism: action_request evaluated by gate() against task_grant producing gate_decision** — gate_function, task_grant, action_request, gate_decision [EXTRACTED 0.95]
- **Digest-Bound Approval System: immutable_draft_artifact hashed to payload_digest and target_digest bound in approval_record** — immutable_draft_artifact, payload_digest, target_digest, approval_record [EXTRACTED 0.92]
- **Authority Materialization Pipeline: deny_by_default authority_composition produces task_grant consumed by gate_function** — deny_by_default, authority_composition, task_grant, gate_function [INFERRED 0.88]
- **OpenSpine Authority Enforcement Pipeline** — authority_composition_concept, task_grant, gate_function, deny_by_default [EXTRACTED 0.95]
- **Email Privacy Containment Pattern** — selected_thread_token, model_gateway, draft_preview_artifact, gate_function [INFERRED 0.85]
- **OpenSpine Implementation Slice Sequence** — core_runtime_schemas_proposal, authority_composition_proposal, telegram_owner_control_proposal, selected_thread_email_proposal, digest_bound_approval_proposal [EXTRACTED 0.90]

## Communities

### Community 0 - "Community 0"
Cohesion: 0.16
Nodes (40): Action Request Type, Agent Manifest Schema, Artifact Ref (protected reference for private payloads), Audit Event, Gate Audit Metadata, Bundle: Core Runtime Schemas Spec, OpenSpine Full OpenSpec Conflux Bundle README, Design: Core Runtime Schemas (+32 more)

### Community 1 - "Community 1"
Cohesion: 0.15
Nodes (26): Authority-Sensitive Change Marking, Proposal: Define Core Runtime Schemas, Proposal: Implement Digest-Bound Draft Approval, Change Layer Classification (Core / Product / Both / Tooling), Main Assistant Agent, OpenSpec/OpenSpine Development Boundary, OpenSpine Development Lifecycle (explore→propose→spec→apply→archive), Design: OpenSpine Development Process (+18 more)

### Community 2 - "Community 2"
Cohesion: 0.21
Nodes (24): OpenSpine Remaining OpenSpec Bundle, OpenSpine Change Backlog, Change: define-core-runtime-schemas, Change: define-openspine-development-process (baseline), Change: define-openspine-development-process, Change: Define Dev Process — Design, Change: Define Dev Process — Proposal, Change: Define Dev Process — Tasks (+16 more)

### Community 3 - "Community 3"
Cohesion: 0.17
Nodes (18): Approval-Required Overrides Plain Allow, Authority Composer (produces task grants from intersected sources), Authority Composition, Authority Composition (Runtime Concept), Design: Authority Composition, Proposal: Implement Authority Composition, Spec: Authority Composition, Tasks: Implement Authority Composition (+10 more)

### Community 4 - "Community 4"
Cohesion: 0.29
Nodes (16): Change: implement-selected-thread-email-preview-slice, Draft Preview Artifact, email_reply_drafter Specialist Workflow, email.thread.selected Event Envelope, Gmail / Google Workspace Owner-Mailbox Connector, Model Gateway, Spec: Selected-Thread Email Preview Slice, Rationale: Email Limited to Selected Thread to Reduce Risk of Broad Inbox Access (+8 more)

### Community 5 - "Community 5"
Cohesion: 0.41
Nodes (15): Approval Record Schema, Bundle: Digest-Bound Draft Approval Design, Bundle: Digest-Bound Draft Approval Proposal, Bundle: Digest-Bound Draft Approval Spec, Bundle: Digest-Bound Draft Approval Tasks, Design: Digest-Bound Draft Approval, Digest-Bound Approval Record, Tasks: Implement Digest-Bound Draft Approval (+7 more)

### Community 6 - "Community 6"
Cohesion: 0.35
Nodes (12): AGENTS.md — Agent Instructions, CLAUDE.md — Claude Instructions, Graphify Knowledge Graph Tool, OpenSpine Review Bundle, Repo Index (Markdown), Repo Index (Plain Text), Skill: openspec-apply-change, Skill: openspec-archive-change (+4 more)

### Community 7 - "Community 7"
Cohesion: 0.33
Nodes (9): Bundle: Define Core Runtime Schemas Proposal, Capability Pack, Digest-Bound Approval, OpenSpec Conventions, Spec: OpenSpine Development Process, OpenSpec Change Management Layer, OpenSpine Governed Runtime Substrate, Rationale: Approval Must Bind Exact Payload Digest (+1 more)

### Community 8 - "Community 8"
Cohesion: 0.67
Nodes (1): main()

## Knowledge Gaps
- **17 isolated node(s):** `Repo Index (Plain Text)`, `Rationale: Approval Must Bind Exact Payload Digest`, `Requirement: Every Change Must Classify Its Affected Layer`, `Rationale: OpenSpec Artifacts Must Not Activate Runtime Authority`, `Change: define-openspine-development-process (baseline)` (+12 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **Thin community `Community 8`** (3 nodes): `main.rs`, `main.rs`, `main()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `gate() — Action Mediation Function` connect `Community 0` to `Community 1`, `Community 2`, `Community 3`, `Community 4`, `Community 5`, `Community 7`?**
  _High betweenness centrality (0.302) - this node is a cross-community bridge._
- **Why does `Task Grant (Live Authority Object)` connect `Community 0` to `Community 1`, `Community 2`, `Community 3`, `Community 4`, `Community 5`, `Community 7`?**
  _High betweenness centrality (0.173) - this node is a cross-community bridge._
- **Why does `OpenSpec (Development/Change-Management Layer)` connect `Community 2` to `Community 6`?**
  _High betweenness centrality (0.132) - this node is a cross-community bridge._
- **Are the 4 inferred relationships involving `gate() — Action Mediation Function` (e.g. with `Action Request Type` and `Gate Decision Type`) actually correct?**
  _`gate() — Action Mediation Function` has 4 INFERRED edges - model-reasoned connections that need verification._
- **Are the 2 inferred relationships involving `Task Grant (Live Authority Object)` (e.g. with `Owner-Control Task Grant` and `Authority Composition`) actually correct?**
  _`Task Grant (Live Authority Object)` has 2 INFERRED edges - model-reasoned connections that need verification._
- **Are the 6 inferred relationships involving `Approval Record Schema` (e.g. with `gate() — Action Mediation Function` and `Selected-Thread Selection Token`) actually correct?**
  _`Approval Record Schema` has 6 INFERRED edges - model-reasoned connections that need verification._
- **Are the 4 inferred relationships involving `Selected-Thread Selection Token` (e.g. with `Digest-Bound Approval` and `Approval Record Schema`) actually correct?**
  _`Selected-Thread Selection Token` has 4 INFERRED edges - model-reasoned connections that need verification._