# Graph Report - openspine  (2026-07-02)

## Corpus Check
- 46 files · ~44,693 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 649 nodes · 1553 edges · 24 communities detected
- Extraction: 78% EXTRACTED · 22% INFERRED · 0% AMBIGUOUS · INFERRED: 346 edges (avg confidence: 0.81)
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
- [[_COMMUNITY_Community 20|Community 20]]
- [[_COMMUNITY_Community 21|Community 21]]
- [[_COMMUNITY_Community 22|Community 22]]
- [[_COMMUNITY_Community 23|Community 23]]

## God Nodes (most connected - your core abstractions)
1. `gate() — Action Mediation Function` - 48 edges
2. `Task Grant (Live Authority Object)` - 41 edges
3. `owner_route()` - 24 edges
4. `owner_event()` - 23 edges
5. `Audit Event` - 19 edges
6. `Approval Record Schema` - 19 edges
7. `gate()` - 18 edges
8. `Store` - 18 edges
9. `main()` - 17 edges
10. `request_for()` - 17 edges

## Surprising Connections (you probably didn't know these)
- `Repo Index (Markdown)` --semantically_similar_to--> `Repo Index (Plain Text)`  [INFERRED] [semantically similar]
  repo-index.md → repo-index.txt
- `Deny-by-Default Authority Policy` --semantically_similar_to--> `Authority-Sensitive Change Marking`  [INFERRED] [semantically similar]
  openspine-full-openspec-conflux-bundle/openspec/specs/authority-composition/spec.md → openspec/changes/define-openspine-development-process/design.md
- `Skills Index (Markdown)` --semantically_similar_to--> `Skills Index (Plain Text)`  [INFERRED] [semantically similar]
  skills-index.md → skills-index.txt
- `Selected-Thread Selection Token` --semantically_similar_to--> `Approval Record Schema`  [INFERRED] [semantically similar]
  openspine-full-openspec-conflux-bundle/openspec/specs/core-runtime-schemas/spec.md → openspine-remaining-openspec-bundle/openspec/changes/define-core-runtime-schemas/design.md
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
Cohesion: 0.08
Nodes (93): Action Request Type, Agent Manifest Schema, Approval Record Schema, Approval-Required Overrides Plain Allow, Artifact Ref (protected reference for private payloads), Audit Event, Gate Audit Metadata, Authority Composer (produces task grants from intersected sources) (+85 more)

### Community 1 - "Community 1"
Cohesion: 0.09
Nodes (60): empty_session_policy(), global_policy(), owner_control_conversation_workflow(), owner_control_input(), owner_identity(), AgentLimits, AgentManifest, main_assistant_agent() (+52 more)

### Community 2 - "Community 2"
Cohesion: 0.05
Nodes (23): ApprovalDecision, ApprovalRecord, matches_rejects_expired_approval(), matches_rejects_non_approved_decisions(), matches_requires_both_digests_and_approved_decision(), round_trips_through_serde(), sample_approval(), TimeoutBehavior (+15 more)

### Community 3 - "Community 3"
Cohesion: 0.09
Nodes (38): allow_reply(), allow_result(), approval_required_on_primary_action_exits_ok_no_reply(), cmd_freeform(), cmd_propose(), cmd_setup(), cmd_status(), deny_on_model_generate_exits_ok() (+30 more)

### Community 4 - "Community 4"
Cohesion: 0.11
Nodes (40): artifact_ref(), email_event(), email_reply_drafter_agent(), email_route(), main_assistant_agent(), owner_control_basic_pack(), owner_event(), owner_route() (+32 more)

### Community 5 - "Community 5"
Cohesion: 0.12
Nodes (31): approval_required_action_stops_before_dispatch(), email_read_inbox_is_denied_for_owner_control_grant(), host_filesystem_read_and_write_are_denied_for_owner_control_grant(), network_raw_egress_is_denied_for_owner_control_grant(), post_action(), start_server(), telegram_reply_is_sent_to_grant_bound_chat(), telegram_reply_payload_rejects_chat_id_override() (+23 more)

### Community 6 - "Community 6"
Cohesion: 0.07
Nodes (31): ArtifactLoadError, ArtifactRegistry, load_registry(), load_yaml_dir(), loads_every_real_fixture_without_error(), malformed_fixture_fails_to_load(), missing_directory_is_not_an_error(), non_yaml_files_are_ignored() (+23 more)

### Community 7 - "Community 7"
Cohesion: 0.08
Nodes (25): ActionRequestBody, ActionResponseBody, dispatch_allowed_action(), DispatchError, post_actions(), TelegramReplyPayload, GenerateRequestBody, GenerateResponseBody (+17 more)

### Community 8 - "Community 8"
Cohesion: 0.17
Nodes (19): ArtifactStore, ArtifactStoreError, different_content_is_different_ref(), get_is_idempotent(), key(), round_trips_plaintext(), same_content_is_content_addressed(), stored_blob_never_contains_the_plaintext_substring() (+11 more)

### Community 9 - "Community 9"
Cohesion: 0.18
Nodes (26): OpenSpine Remaining OpenSpec Bundle, OpenSpine Change Backlog, Change: define-core-runtime-schemas, Change: define-openspine-development-process (baseline), Change: define-openspine-development-process, Change: Define Dev Process — Design, Change: Define Dev Process — Proposal, Change: Define Dev Process — Tasks (+18 more)

### Community 10 - "Community 10"
Cohesion: 0.16
Nodes (24): Authority-Sensitive Change Marking, Proposal: Define Core Runtime Schemas, Change Layer Classification (Core / Product / Both / Tooling), Main Assistant Agent, OpenSpec/OpenSpine Development Boundary, OpenSpine Development Lifecycle (explore→propose→spec→apply→archive), Design: OpenSpine Development Process, Proposal: Define OpenSpine Development Process (+16 more)

### Community 11 - "Community 11"
Cohesion: 0.11
Nodes (19): AccountRole, actor_hint_defaults_to_all_none(), ActorHint, ChannelTrust, Connector, DataClassification, EventEnvelope, EventType (+11 more)

### Community 12 - "Community 12"
Cohesion: 0.18
Nodes (14): ActionBody, ActionOutcome, approval_required_is_ok_not_err(), deny_decision_is_ok_not_err(), generate_sends_bearer_auth(), GenerateBody, get_task_sends_bearer_auth(), KernelClient (+6 more)

### Community 13 - "Community 13"
Cohesion: 0.16
Nodes (9): docker_driver_args_are_correct_and_secret_free(), DockerDriver, process_driver_allows_external_communication_with_explicit_opt_in(), process_driver_clears_env_and_sets_only_two_vars(), process_driver_never_refuses_owner_control_lane(), process_driver_refuses_external_communication_without_opt_in(), ProcessDriver, refuses_external_communication_without_containment() (+1 more)

### Community 14 - "Community 14"
Cohesion: 0.32
Nodes (11): anthropic_client_parses_the_reply_text(), GatewayError, generate_anthropic(), generate_openai_compat(), http_client(), malformed_response_is_missing_content_not_a_panic(), messages_json(), openai_compat_client_parses_the_reply_text() (+3 more)

### Community 15 - "Community 15"
Cohesion: 0.18
Nodes (13): deny_unknown_fields_rejects_capability_pack_id(), EntityType, Identifier, IdentifierKind, IdentifierVerificationMethod, Identity, identity_json_has_no_authority_field(), IdentityResolution (+5 more)

### Community 16 - "Community 16"
Cohesion: 0.35
Nodes (12): AGENTS.md — Agent Instructions, CLAUDE.md — Claude Instructions, Graphify Knowledge Graph Tool, OpenSpine Review Bundle, Repo Index (Markdown), Repo Index (Plain Text), Skill: openspec-apply-change, Skill: openspec-archive-change (+4 more)

### Community 17 - "Community 17"
Cohesion: 0.2
Nodes (8): InstructionSources, ModelRequest, OutputPolicy, Provider, RedactionRequirement, RetentionMode, round_trips_through_serde(), StoreOutputPolicy

### Community 18 - "Community 18"
Cohesion: 0.24
Nodes (7): round_trips_through_serde(), sample_token(), SelectionScope, SelectionToken, SelectionTokenType, SelectionVerificationMethod, single_use_defaults_to_true_when_omitted()

### Community 19 - "Community 19"
Cohesion: 0.22
Nodes (2): ArtifactRef, Lifecycle

### Community 20 - "Community 20"
Cohesion: 0.36
Nodes (6): authority_sources_use_kind_id_version_format(), GrantLimits, is_expired_uses_expires_at(), owner_control_grant(), round_trips_through_serde(), TaskGrant

### Community 21 - "Community 21"
Cohesion: 0.4
Nodes (5): Rationale: OpenSpec Artifacts Must Not Activate Runtime Authority, Requirement: Authority-Sensitive Changes Must Be Explicitly Marked, Requirement: Every Change Must Classify Its Affected Layer, Requirement: OpenSpec Must Remain Separate from Runtime Authority, Spec: OpenSpine Development Process

### Community 22 - "Community 22"
Cohesion: 0.5
Nodes (1): WorkflowManifest

### Community 23 - "Community 23"
Cohesion: 0.67
Nodes (1): Store

## Knowledge Gaps
- **115 isolated node(s):** `TaskLimits`, `TaskView`, `ActionOutcome`, `ModelOutcome`, `ActionBody` (+110 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **Thin community `Community 19`** (9 nodes): `artifact.rs`, `artifact_ref_rejects_unknown_fields()`, `ArtifactRef`, `can_transition()`, `default_version()`, `happy_path_chain_is_legal()`, `Lifecycle`, `no_skipping_stages()`, `terminal_states_have_no_outgoing_transitions()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 22`** (4 nodes): `workflow.rs`, `action_lists_default_to_empty_when_omitted()`, `round_trips_through_serde()`, `WorkflowManifest`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 23`** (3 nodes): `Store`, `.approval_for_request()`, `.find_selection_token()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `post_model_generate()` connect `Community 7` to `Community 2`, `Community 3`, `Community 4`?**
  _High betweenness centrality (0.390) - this node is a cross-community bridge._
- **Why does `Model Gateway` connect `Community 0` to `Community 7`?**
  _High betweenness centrality (0.356) - this node is a cross-community bridge._
- **Why does `gate() — Action Mediation Function` connect `Community 0` to `Community 9`, `Community 10`?**
  _High betweenness centrality (0.114) - this node is a cross-community bridge._
- **Are the 4 inferred relationships involving `gate() — Action Mediation Function` (e.g. with `Action Request Type` and `Gate Decision Type`) actually correct?**
  _`gate() — Action Mediation Function` has 4 INFERRED edges - model-reasoned connections that need verification._
- **Are the 2 inferred relationships involving `Task Grant (Live Authority Object)` (e.g. with `Owner-Control Task Grant` and `Authority Composition`) actually correct?**
  _`Task Grant (Live Authority Object)` has 2 INFERRED edges - model-reasoned connections that need verification._
- **Are the 11 inferred relationships involving `owner_route()` (e.g. with `owner_control_grant_matches_prd_12_1()` and `no_candidate_allow_means_action_is_not_granted()`) actually correct?**
  _`owner_route()` has 11 INFERRED edges - model-reasoned connections that need verification._
- **Are the 12 inferred relationships involving `owner_event()` (e.g. with `owner_control_grant_matches_prd_12_1()` and `no_candidate_allow_means_action_is_not_granted()`) actually correct?**
  _`owner_event()` has 12 INFERRED edges - model-reasoned connections that need verification._