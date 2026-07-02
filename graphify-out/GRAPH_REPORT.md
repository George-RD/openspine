# Graph Report - openspine  (2026-07-02)

## Corpus Check
- 26 files · ~25,967 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 388 nodes · 951 edges · 20 communities detected
- Extraction: 81% EXTRACTED · 19% INFERRED · 0% AMBIGUOUS · INFERRED: 179 edges (avg confidence: 0.81)
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
- [[_COMMUNITY_Community 9|Community 9]]
- [[_COMMUNITY_Community 10|Community 10]]
- [[_COMMUNITY_Community 11|Community 11]]
- [[_COMMUNITY_Community 12|Community 12]]
- [[_COMMUNITY_Community 13|Community 13]]
- [[_COMMUNITY_Community 14|Community 14]]
- [[_COMMUNITY_Community 15|Community 15]]
- [[_COMMUNITY_Community 16|Community 16]]
- [[_COMMUNITY_Community 17|Community 17]]
- [[_COMMUNITY_Community 18|Community 18]]
- [[_COMMUNITY_Community 19|Community 19]]

## God Nodes (most connected - your core abstractions)
1. `gate() — Action Mediation Function` - 48 edges
2. `Task Grant (Live Authority Object)` - 41 edges
3. `owner_route()` - 23 edges
4. `owner_event()` - 22 edges
5. `Audit Event` - 19 edges
6. `Approval Record Schema` - 19 edges
7. `Selected-Thread Selection Token` - 17 edges
8. `Deny-by-Default Authority Policy` - 16 edges
9. `Design: Core Runtime Schemas` - 16 edges
10. `compose_authority()` - 14 edges

## Surprising Connections (you probably didn't know these)
- `Repo Index (Markdown)` --semantically_similar_to--> `Repo Index (Plain Text)`  [INFERRED] [semantically similar]
  repo-index.md → repo-index.txt
- `Selected-Thread Selection Token` --semantically_similar_to--> `Approval Record Schema`  [INFERRED] [semantically similar]
  openspine-full-openspec-conflux-bundle/openspec/specs/core-runtime-schemas/spec.md → openspine-remaining-openspec-bundle/openspec/changes/define-core-runtime-schemas/design.md
- `Selected-Thread Selection Token` --semantically_similar_to--> `Selected-Thread Token`  [INFERRED] [semantically similar]
  openspine-full-openspec-conflux-bundle/openspec/specs/core-runtime-schemas/spec.md → openspec/changes/implement-selected-thread-email-preview-slice/design.md
- `Skills Index (Markdown)` --semantically_similar_to--> `Skills Index (Plain Text)`  [INFERRED] [semantically similar]
  skills-index.md → skills-index.txt
- `Selected-Thread Selection Token` --semantically_similar_to--> `Route Artifact Schema`  [INFERRED] [semantically similar]
  openspine-full-openspec-conflux-bundle/openspec/specs/core-runtime-schemas/spec.md → openspine-remaining-openspec-bundle/openspec/changes/define-core-runtime-schemas/design.md

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
Cohesion: 0.11
Nodes (63): Action Request Type, Agent Manifest Schema, Artifact Ref (protected reference for private payloads), Audit Event, Gate Audit Metadata, Authority Composition, Bundle: Define Core Runtime Schemas Proposal, Bundle: Core Runtime Schemas Spec (+55 more)

### Community 1 - "Community 1"
Cohesion: 0.12
Nodes (52): artifact_ref(), email_event(), email_reply_drafter_agent(), email_route(), empty_session_policy(), global_policy(), owner_control_conversation_workflow(), owner_control_input() (+44 more)

### Community 2 - "Community 2"
Cohesion: 0.1
Nodes (37): Approval-Required Overrides Plain Allow, Authority Composer (produces task grants from intersected sources), Authority Composition (Runtime Concept), Design: Authority Composition, Proposal: Implement Authority Composition, Spec: Authority Composition, Tasks: Implement Authority Composition, Authority-Sensitive Change Marking (+29 more)

### Community 3 - "Community 3"
Cohesion: 0.08
Nodes (18): ApprovalDecision, ApprovalRecord, matches_rejects_expired_approval(), matches_rejects_non_approved_decisions(), matches_requires_both_digests_and_approved_decision(), round_trips_through_serde(), sample_approval(), TimeoutBehavior (+10 more)

### Community 4 - "Community 4"
Cohesion: 0.19
Nodes (27): Approval Record Schema, Bundle: Digest-Bound Draft Approval Design, Bundle: Digest-Bound Draft Approval Proposal, Bundle: Digest-Bound Draft Approval Spec, Bundle: Digest-Bound Draft Approval Tasks, Design: Digest-Bound Draft Approval, Proposal: Implement Digest-Bound Draft Approval, Digest-Bound Approval Record (+19 more)

### Community 5 - "Community 5"
Cohesion: 0.11
Nodes (18): AccountRole, ActorHint, ChannelTrust, Connector, DataClassification, EventEnvelope, EventType, InteractionMode (+10 more)

### Community 6 - "Community 6"
Cohesion: 0.23
Nodes (21): OpenSpine Remaining OpenSpec Bundle, OpenSpine Change Backlog, Change: define-core-runtime-schemas, Change: define-openspine-development-process (baseline), Change: define-openspine-development-process, Change: Define Dev Process — Design, Change: Define Dev Process — Proposal, Change: Define Dev Process — Tasks (+13 more)

### Community 7 - "Community 7"
Cohesion: 0.18
Nodes (13): deny_unknown_fields_rejects_capability_pack_id(), EntityType, Identifier, IdentifierKind, IdentifierVerificationMethod, Identity, identity_json_has_no_authority_field(), IdentityResolution (+5 more)

### Community 8 - "Community 8"
Cohesion: 0.17
Nodes (6): action_id_qualifier_is_part_of_identity(), action_id_serializes_as_bare_string(), ActionId, ActionRequest, DenialReason, GateDecision

### Community 9 - "Community 9"
Cohesion: 0.17
Nodes (8): effect_defaults_to_allow_when_omitted(), round_trips_through_serde(), Route, route_can_be_a_deny_route(), RouteActorWhen, RouteEffect, RouteResolution, RouteWhen

### Community 10 - "Community 10"
Cohesion: 0.35
Nodes (12): AGENTS.md — Agent Instructions, CLAUDE.md — Claude Instructions, Graphify Knowledge Graph Tool, OpenSpine Review Bundle, Repo Index (Markdown), Repo Index (Plain Text), Skill: openspec-apply-change, Skill: openspec-archive-change (+4 more)

### Community 11 - "Community 11"
Cohesion: 0.35
Nodes (10): agent_manifests_round_trip(), artifacts_dir(), email_grant_pack_excludes_read_inbox_and_send(), every_fixture_file_is_covered_by_a_test(), global_policy_round_trips_and_denies_send(), owner_control_pack_round_trips(), owner_email_selected_thread_route_is_expressible_declaratively(), owner_telegram_route_is_expressible_declaratively() (+2 more)

### Community 12 - "Community 12"
Cohesion: 0.24
Nodes (7): round_trips_through_serde(), sample_token(), SelectionScope, SelectionToken, SelectionTokenType, SelectionVerificationMethod, single_use_defaults_to_true_when_omitted()

### Community 13 - "Community 13"
Cohesion: 0.2
Nodes (8): InstructionSources, ModelRequest, OutputPolicy, Provider, RedactionRequirement, RetentionMode, round_trips_through_serde(), StoreOutputPolicy

### Community 14 - "Community 14"
Cohesion: 0.22
Nodes (2): ArtifactRef, Lifecycle

### Community 15 - "Community 15"
Cohesion: 0.22
Nodes (8): AgentLimits, AgentManifest, main_assistant_denies_broad_email_access(), MemoryScope, ModelPolicy, OutputChannels, Persistence, round_trips_through_serde()

### Community 16 - "Community 16"
Cohesion: 0.36
Nodes (6): authority_sources_use_kind_id_version_format(), GrantLimits, is_expired_uses_expires_at(), owner_control_grant(), round_trips_through_serde(), TaskGrant

### Community 17 - "Community 17"
Cohesion: 0.33
Nodes (3): Constraints, Policy, SessionPolicy

### Community 18 - "Community 18"
Cohesion: 0.5
Nodes (1): WorkflowManifest

### Community 19 - "Community 19"
Cohesion: 0.67
Nodes (1): main()

## Knowledge Gaps
- **78 isolated node(s):** `AuthorityInput`, `AuthorityOutcome`, `Constraints`, `Policy`, `SessionPolicy` (+73 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **Thin community `Community 14`** (9 nodes): `artifact.rs`, `artifact_ref_rejects_unknown_fields()`, `ArtifactRef`, `can_transition()`, `default_version()`, `happy_path_chain_is_legal()`, `Lifecycle`, `no_skipping_stages()`, `terminal_states_have_no_outgoing_transitions()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 18`** (4 nodes): `workflow.rs`, `action_lists_default_to_empty_when_omitted()`, `round_trips_through_serde()`, `WorkflowManifest`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 19`** (3 nodes): `main.rs`, `main.rs`, `main()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `gate() — Action Mediation Function` connect `Community 0` to `Community 2`, `Community 4`, `Community 6`?**
  _High betweenness centrality (0.055) - this node is a cross-community bridge._
- **Why does `sample_envelope()` connect `Community 5` to `Community 1`?**
  _High betweenness centrality (0.041) - this node is a cross-community bridge._
- **Why does `owner_route()` connect `Community 1` to `Community 9`?**
  _High betweenness centrality (0.034) - this node is a cross-community bridge._
- **Are the 4 inferred relationships involving `gate() — Action Mediation Function` (e.g. with `Action Request Type` and `Gate Decision Type`) actually correct?**
  _`gate() — Action Mediation Function` has 4 INFERRED edges - model-reasoned connections that need verification._
- **Are the 2 inferred relationships involving `Task Grant (Live Authority Object)` (e.g. with `Owner-Control Task Grant` and `Authority Composition`) actually correct?**
  _`Task Grant (Live Authority Object)` has 2 INFERRED edges - model-reasoned connections that need verification._
- **Are the 10 inferred relationships involving `owner_route()` (e.g. with `owner_control_grant_matches_prd_12_1()` and `no_candidate_allow_means_action_is_not_granted()`) actually correct?**
  _`owner_route()` has 10 INFERRED edges - model-reasoned connections that need verification._
- **Are the 11 inferred relationships involving `owner_event()` (e.g. with `owner_control_grant_matches_prd_12_1()` and `no_candidate_allow_means_action_is_not_granted()`) actually correct?**
  _`owner_event()` has 11 INFERRED edges - model-reasoned connections that need verification._