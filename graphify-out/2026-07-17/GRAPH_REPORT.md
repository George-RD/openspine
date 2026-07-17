# Graph Report - implement-overlay-model  (2026-07-17)

## Corpus Check
- 390 files · ~286,537 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 5044 nodes · 7053 edges · 896 communities (394 shown, 502 thin omitted)
- Extraction: 88% EXTRACTED · 12% INFERRED · 0% AMBIGUOUS · INFERRED: 844 edges (avg confidence: 0.8)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `796c8ec4`
- Run `git rev-parse HEAD` and compare to check if the graph is stale.
- Run `graphify update .` after code changes (no API cost).

## Community Hubs (Navigation)
- README.md
- .new
- event.rs
- handle_owner_update
- .default
- GmailConnector
- artifact_loader.rs
- config.rs
- mod.rs
- telegram.rs
- .put
- ProposedArtifact
- client.rs
- policy.rs
- actions.rs
- ADDED Requirements
- Requirements
- content.d.ts
- AGENTS.md — Agent Instructions
- action.rs
- StoreError
- post_action
- ApprovalRecord
- ActionId
- AppState
- AppState
- .sweep_expired_grants
- Lifecycle
- Requirements
- OpenSpine Agent-OS Design Log
- ArtifactRef
- ADDED Requirements
- Requirements
- sandbox.rs
- ADDED Requirements
- Requirements
- Requirements
- Requirements
- Requirements
- properties
- ADDED Requirements
- digest.rs
- Digest
- Design: OpenSpine development process
- ADDED Requirements
- ADDED Requirements
- mod.rs
- ADDED Requirements
- ADDED Requirements
- Requirements
- Requirements
- Requirements
- owner_event
- .default
- Runtime schema groups
- ADDED Requirements
- ADDED Requirements
- ADDED Requirements
- ADDED Requirements
- Requirements
- scripts
- compose_authority
- ConnectorRegistry
- identity.rs
- properties
- tests.rs
- Proposal: Define OpenSpine development process
- MODIFIED Requirements
- properties
- create_approved_draft
- .put
- SKILL.md
- Tasks: Harden approval and budgets
- properties
- explore.md
- OpenSpine kernel↔shell HTTP contract
- Ok
- Proposal: Define core runtime schemas
- Proposal: Implement authority composition
- Proposal: Implement digest-bound draft approval
- Proposal: Implement gate action API
- Proposal: Implement selected-thread email preview slice
- Tasks: Implement selected-thread email preview slice
- Proposal: Implement Telegram owner control slice
- Proposal: Backfill implemented capability specs
- Proposal: Harden approval and budgets
- Proposal: Implement artifact lifecycle slice
- Tasks: Implement artifact lifecycle slice
- Agent-OS change sequence (2026-07-07, AD canon)
- AuditMeta
- AuditEvent
- Tasks: Define core runtime schemas
- Tasks: Define OpenSpine development process
- Tasks: Implement digest-bound draft approval
- Tasks: Implement Telegram owner control slice
- artifact_activation_tests.rs
- Design: Authority composition
- Tasks: Backfill implemented capability specs
- ADDED Requirements
- ADDED Requirements
- OpenSpine conventions
- openspine-decision-log.md
- D-051 — The agent-OS canon (AD-001..153) is decomposed into a dependency-edged change sequence; the stale later-changes placeholders are superseded or subsumed
- OpenSpine
- Gmail selected-thread email preview setup (Phase 2)
- Telegram owner-control setup (Phase 1)
- Tasks: Implement authority composition
- Design: Digest-bound draft approval
- Design: Gate action API
- Tasks: Implement gate action API
- Design: Selected-thread email preview slice
- Design: Telegram owner control slice
- Requirement: Selection tokens MUST be single-use, expiring, and scope-fixed
- D-063 — Model-swap activation uses a serialized staged recovery protocol
- Kernel foundation
- attrs
- Design: Harden approval and budgets
- Failure surfacing & operations
- D-008 — Deterministic routing decides authority; agentic routing decides strategy
- D-001 — Lyra is a runtime/substrate, not a single agent
- mod.rs
- D-042 — Reply recipient is kernel-derived, never shell-supplied: newest non-owner sender, matched against a configured mailbox address
- D-002 — First usable UX should include an owner control channel
- D-003 — Gmail is a guarded workflow, not the whole product
- D-004 — Every effectful action goes through `gate()`
- D-005 — Private-data shell must be contained
- D-009 — External content is data, not instruction
- D-021 — Email domain is broader than Gmail
- D-022 — Agent-owned inbox is distinct from owner mailbox access
- D-023 — OpenSpine is the substrate; Lyra is a product built on it
- D-024 — OpenSpec is the development/change-management layer, not the runtime
- D-026 — Shell containment via a `SandboxDriver` trait (Process dev-only / Docker)
- banner
- super::Store
- add_column_if_missing
- Design: Backfill implemented capability specs
- Design: Artifact lifecycle slice
- Delegation & containment
- Skills & workflows
- Reflection & product surface
- package.json
- D-008 — Deterministic routing decides authority; agentic routing decides strategy
- D-037 — Gmail OAuth via a plain refresh-token POST (no `oauth2` crate); `base64` promoted from transitive to direct dependency
- model_swap.rs
- D-040 — Pending (pre-approval) `ActionRequest`s are persisted in a new `action_requests` table
- D-041 — `email.create_draft`'s digest composition: payload = `{subject, body}`, target = `{thread_id, connector, account_role, recipients}`
- D-043 — `lyra.ui.preview` is extended (not duplicated) to propose the exact reviewed draft and attach the approval button
- D-044 — Approved draft creation dispatches kernel-side; no new shell spawn
- D-045 — WYSIWYS: a truncated preview refuses an approval button rather than splitting the message
- D-046 — Grant budgets are enforced kernel-dispatch-side; the artifact budget counts only shell-initiated puts
- D-047 — Task tokens are hashed at rest; expired grants are swept
- D-048 — `artifact.activate` is the single canonical activation action id; every runtime proposal requires uniform owner approval; prompt templates are excluded from proposable kinds
- D-049 — Capability specs are backfilled for subsystems implemented inside earlier slices
- D-050 — `max_model_calls` is enforced with an atomic upsert, not a count-then-compare
- D-006 — Identity is not authority
- D-011 — Approval must be digest-bound
- D-012 — Audit stores private payloads by encrypted/hash reference
- D-013 — Dynamic behavior easy; dynamic authority hard
- D-014 — Bootstrap/setup secrets bypass shell/model context
- D-015 — Phase 1 should avoid final email send
- D-016 — Capability packs are candidate profiles, not live authority
- D-017 — Personas grant no authority
- D-018 — Routes are declarative artifacts, not kernel code
- D-020 — Railway/Docker/VPS are deployment targets, not core architecture
- D-025 — Rust/Tokio substrate: storage, audit chain, and secrets handling
- D-027 — Multi-provider model gateway with per-provider auth mode
- D-028 — Canonical artifact format is YAML+serde; digests are `sha256:` over canonical JSON
- D-029 — Gmail OAuth scopes: `readonly` + `compose`, send hard-denied at the gate
- openspine-decision-log.md
- D-031 — Docker Compose is the first reference deployment target
- D-032 — Kernel↔shell transport is HTTP/JSON with a per-task bearer token
- D-033 — Action identifiers are exact-match dotted strings; unverified senders are audited and ignored
- D-034 — `email.create_draft` is the one canonical action id; the qualified PRD §10.2 spelling is dropped
- D-035 — Kernel advertises a separate `advertise_endpoint` from its `bind_addr`; no Unix-domain-socket transport for `ProcessDriver`
- D-010 — Model calls with private context go through model gateway
- D-019 — Implement minimal slice first, not full agent OS
- why-openspine.md
- Q: Trace the full runtime call path for a single incoming Telegram message from an authorized owner, all the way through to a TaskGrant being persisted in the kernel's SQLite store. List the modules/files involved, in call order, and identify the 2-3 modules that are most structurally central to this flow.
- Blindspot pass
- Brainstorm and prototypes
- Change quiz
- Implementation notes
- Implementation plan
- Interview me
- Pitch packager
- Reference hunt
- template
- architecture.md
- decisions.md
- quickstart.md
- roadmap.md
- threat-model.md
- tsconfig.json
- editUrl
- head
- pagefind
- AGENTS.md
- AGENTS.md
- proposal.rs
- autoresearch.sh
- CLAUDE.md
- README.md
- check.sh
- check-claims.sh
- check-file-sizes.sh
- README.md
- content.config.ts
- index.mdx
- Agent Manifest Schema
- apply.md
- archive.md
- propose.md
- CLAUDE.md — Claude Instructions
- SKILL.md
- SKILL.md
- SKILL.md
- SKILL.md
- SKILL.md
- SKILL.md
- lib.rs
- ids.rs
- lib.rs
- email.create_draft Action
- email.send Action (Denied)
- Graphify Knowledge Graph Tool
- opsx-apply.md
- opsx-archive.md
- opsx-propose.md
- SKILL.md
- SKILL.md
- SKILL.md
- Spec: Authority Composition
- OpenSpec Change Sequence
- Spec: Core Runtime Schemas
- Spec: OpenSpine Development Process
- Spec: Digest-Bound Draft Approval
- Spec: Gate Action API
- OpenSpec Change Management Layer
- Spec: Selected-Thread Email Preview Slice
- Spec: Telegram Owner Control Slice
- OpenSpine Governed Runtime Substrate
- Repo Index (Plain Text)
- content-assets.mjs
- content-modules.mjs
- types.d.ts
- Skills Index (Plain Text)
- telegram.owner.message Event Type
- Workflow Manifest Schema
- SKILL.md
- opsx-explore.md
- opsx-apply.md
- opsx-archive.md
- opsx-propose.md
- SKILL.md
- SKILL.md
- SKILL.md
- TaskGrant
- ADDED Requirements
- Proposal: Refactor kernel registries
- Approach
- Tasks: Refactor kernel registries
- ApprovalRecord
- fixtures.rs
- D-052 — Archive applies deltas mechanically via `openspec archive --yes`; pre-seeded requirements are carried as MODIFIED; the `--skip-specs` hand-apply ceremony is retired
- D-030 — Telegram carries the entire owner-control UX for phases 1–3
- artifact_propose.rs
- Overlay & key model
- Proposal: Define grant chain and modes
- D-010 — Model calls with private context go through model gateway
- Q: Trace the full runtime call path for a single incoming Telegram message from an authorized owner, all the way through to a TaskGrant being persisted in the kernel's SQLite store. List the modules/files involved, in call order, and identify the 2-3 modules that are most structurally central to this flow.
- Blindspot pass
- Delegation & containment
- SelectionToken
- tasks.md
- Implementation notes
- tests.rs
- get_task
- Approach
- Approach
- action_catalog.rs
- .fmt
- artifact_propose.rs
- effect_paths.rs
- Overlay & key model
- retry_worker.rs
- D-014 — Bootstrap/setup secrets bypass shell/model context
- editUrl
- Proposal: Implement identity store and principal
- Tasks: Implement identity store and principal
- Option
- Ulid
- Vec
- HashMap
- benchmark.rs
- Timestamp
- head
- Vec
- lib.rs
- Arc
- Error
- D-015 — Phase 1 should avoid final email send
- D-016 — Capability packs are candidate profiles, not live authority
- Option
- Result
- State
- StatusCode
- String
- Ulid
- Value
- MockServer
- TelegramConnector
- D-032 — Kernel↔shell transport is HTTP/JSON with a per-task bearer token
- D-033 — Action identifiers are exact-match dotted strings; unverified senders are audited and ignored
- MockServer
- Timestamp
- Value
- Arc
- HeaderMap
- D-030 — Telegram carries the entire owner-control UX for phases 1–3
- Option
- Result
- State
- StatusCode
- String
- Ulid
- Value
- JoinHandle
- Option
- Response
- SocketAddr
- HashMap
- Option
- Self
- Value
- Arc
- Display
- HeaderMap
- Json
- Option
- Result
- State
- StatusCode
- Ulid
- Value
- Vec
- Arc
- HeaderMap
- Json
- Result
- State
- HeaderMap
- Json
- Option
- Result
- State
- StatusCode
- Ulid
- Value
- JoinHandle
- Option
- Response
- HashMap
- Option
- Self
- Value
- Arc
- Display
- HeaderMap
- Json
- Option
- Result
- State
- StatusCode
- Ulid
- Value
- Vec
- Arc
- State
- StatusCode
- String
- Value
- Vec
- String
- JoinHandle
- Option
- HashMap
- Option
- Path
- PathBuf
- Policy
- Result
- String
- Vec
- WorkflowManifest
- PathBuf
- Error
- PathBuf
- Result
- Self
- String
- Vec
- Result
- Error
- Option
- Path
- PathBuf
- Result
- Self
- String
- Vec
- Client
- Error
- Mutex
- Option
- Result
- Self
- String
- Timestamp
- Value
- Vec
- GmailConnector
- MockServer
- Value
- Option
- Result
- Self
- Store
- String
- Ulid
- Option
- String
- Vec
- Client
- Error
- Result
- Self
- String
- Value
- Vec
- Result
- Option
- Result
- String
- Timestamp
- Vec
- Result
- String
- Timestamp
- Ulid
- Value
- MockServer
- Store
- String
- Vec
- Default
- Path
- PathBuf
- Result
- Self
- String
- Vec
- Option
- String
- Timestamp
- Ulid
- gate_support.rs
- Option
- Result
- Ulid
- Error
- Option
- Result
- Ulid
- Option
- Result
- Ulid
- Value
- Vec
- JoinHandle
- Vec
- Option
- ArtifactId
- String
- Vec
- String
- Timestamp
- Ulid
- D
- CapabilityPack
- Error
- F
- Formatter
- Into
- Result
- S
- HashMap
- Self
- String
- Vec
- Client
- ModelSwapManifest
- Option
- Path
- PathBuf
- Policy
- Result
- String
- Vec
- WorkflowManifest
- PathBuf
- Error
- D-031 — Docker Compose is the first reference deployment target
- test_hooks.rs
- Option
- Path
- PathBuf
- Result
- Self
- String
- String
- Vec
- D-032 — Kernel↔shell transport is HTTP/JSON with a per-task bearer token
- D-033 — Action identifiers are exact-match dotted strings; unverified senders are audited and ignored
- Result
- Self
- String
- Value
- D-034 — `email.create_draft` is the one canonical action id; the qualified PRD §10.2 spelling is dropped
- Result
- Option
- Result
- String
- Timestamp
- Vec
- Box
- Result
- Timestamp
- String
- Timestamp
- Ulid
- Value
- Default
- Path
- PathBuf
- Result
- Self
- String
- Vec
- Into
- Option
- Self
- String
- Timestamp
- Ulid
- ArtifactId
- String
- Vec
- Option
- Result
- String
- Value
- Vec
- Option
- Result
- String
- mod.rs
- Result
- String
- opsx-apply.md
- opsx-archive.md
- opsx-propose.md
- SKILL.md
- SKILL.md
- SKILL.md
- opsx-apply.md
- opsx-archive.md
- opsx-propose.md
- SKILL.md
- SKILL.md
- SKILL.md
- Timestamp
- Ulid
- Vec
- lib.rs
- Client
- Option
- Result
- Self
- String
- Value
- Vec
- Option
- PathBuf
- Result
- Error
- Result
- Self
- String
- Value
- Vec
- Result
- Option
- Result
- String
- Formatter
- Result
- State
- StatusCode
- String
- String
- Value
- Option
- String
- Ulid
- Vec
- ids.rs
- Option
- String
- Ulid
- Vec
- ArtifactId
- Option
- String
- Vec
- Ulid
- ArtifactId
- Connector
- Option
- String
- Connector
- Into
- Option
- Self
- String
- Timestamp
- Ulid
- ArtifactId
- Vec
- Option
- Result
- String
- Value
- Vec
- Option
- Result
- String
- Value
- mod.rs
- Result
- String
- opsx-apply.md
- opsx-archive.md
- opsx-propose.md
- SKILL.md
- SKILL.md
- SKILL.md
- opsx-apply.md
- opsx-propose.md
- SKILL.md
- SKILL.md
- SKILL.md
- event_bus_tests.rs
- agent.rs
- eval_verdict_store_tests.rs
- Lifecycle
- policy.rs
- event_bus.rs
- pack.rs
- D
- Deserialize
- Display
- Error
- Formatter
- Into
- Result
- S
- Self
- String
- Value
- Option
- String
- Ulid
- Vec
- ids.rs
- Option
- String
- Ulid
- Vec
- ArtifactId
- Option
- String
- Vec
- Ulid
- ArtifactId
- Connector
- Option
- String
- Connector
- Into
- Option
- Self
- String
- Timestamp
- Ulid
- ArtifactId
- String
- Vec
- Option
- Result
- String
- Value
- Vec
- Option
- Result
- String
- Value
- mod.rs
- Result
- String
- D-010 — Model calls with private context go through model gateway
- D-019 — Implement minimal slice first, not full agent OS
- artifact_propose.rs
- SKILL.md
- SKILL.md
- SKILL.md
- opsx-apply.md
- MockServer
- String
- TelegramConnector
- Value
- MockServer
- Store
- Duration
- pack.rs
- Vec
- MockServer
- audit_support.rs
- Connection
- Option
- Option
- String
- Timestamp
- Ulid
- Vec
- Result
- Ulid
- Option
- Result
- Ulid
- Vec
- Option
- Result
- Timestamp
- Ulid
- Vec
- Connection
- Option
- Result
- Self
- String
- Timestamp
- Ulid
- Vec
- Connection
- Result
- Arc
- Connection
- Error
- Mutex
- Option
- Path
- Result
- Self
- String
- Ulid
- Vec
- test_hooks.rs
- String
- Vec
- Arc
- Mutex
- Option
- Result
- Self
- String
- Timestamp
- Ulid
- Vec
- Option
- Option
- PathBuf
- TelegramConnector
- Store
- ArtifactRef
- D
- Deserialize
- Display
- Error
- Formatter
- Into
- Option
- Result
- Self
- String
- Ulid
- Vec
- Vec
- Duration
- ModelSwapManifest
- Result
- String
- Vec
- Option
- Result
- String
- Vec
- ArtifactRef
- Box
- Result
- Duration
- Option
- Result
- String
- Vec
- Result
- Ulid
- Option
- Result
- Result
- Self
- String
- Connection
- Result
- String
- Transaction
- Vec

## God Nodes (most connected - your core abstractions)
1. `handle_owner_update()` - 76 edges
2. `AppState` - 73 edges
3. `StoreError` - 73 edges
4. `owner_update()` - 67 edges
5. `test_state_with_telegram()` - 51 edges
6. `TaskGrant` - 50 edges
7. `gate()` - 46 edges
8. `ArtifactRegistry` - 46 edges
9. `request_for()` - 33 edges
10. `test_state()` - 33 edges

## Surprising Connections (you probably didn't know these)
- `malformed_fixture_fails_to_load()` --calls--> `load_registry()`  [INFERRED]
  crates/openspine-kernel/src/artifact_loader/tests.rs → crates/openspine-kernel/src/artifact_loader.rs
- `missing_directory_is_not_an_error()` --calls--> `load_registry()`  [INFERRED]
  crates/openspine-kernel/src/artifact_loader/tests.rs → crates/openspine-kernel/src/artifact_loader.rs
- `non_yaml_files_are_ignored()` --calls--> `load_registry()`  [INFERRED]
  crates/openspine-kernel/src/artifact_loader/tests.rs → crates/openspine-kernel/src/artifact_loader.rs
- `kind_table_excludes_templates()` --calls--> `parse_proposal()`  [INFERRED]
  crates/openspine-kernel/src/artifact_loader/tests.rs → crates/openspine-kernel/src/artifact_loader.rs
- `plan_proposal_budget_exhaustion_persists_no_request()` --calls--> `test_state()`  [INFERRED]
  crates/openspine-kernel/src/pipeline/tests/plan.rs → crates/openspine-kernel/src/test_support.rs

## Import Cycles
- 2-file cycle: `crates/openspine-kernel/src/api/handler_registry.rs -> crates/openspine-kernel/src/pipeline/mod.rs -> crates/openspine-kernel/src/api/handler_registry.rs`

## Communities (896 total, 502 thin omitted)

### Community 0 - "README.md"
Cohesion: 0.06
Nodes (49): action_request_consume_is_single_use(), action_request_round_trips_by_id(), approval_round_trips_by_action_request_id(), find_task_grant_by_token_rejects_the_raw_hash_value(), most_recent_approval_wins_when_multiple_exist_for_one_request(), opening_a_pre_existing_db_without_the_used_column_is_migrated_in_place(), persisted_grant_json_contains_no_task_token(), sample_action_request() (+41 more)

### Community 1 - ".new"
Cohesion: 0.06
Nodes (22): Bot, Update, Url, build_owner_envelope(), CallbackQueryUpdate, parse_digest_detail_command(), parse_digest_namespace(), project_update() (+14 more)

### Community 2 - "event.rs"
Cohesion: 0.07
Nodes (67): bound_parameter_conflict_is_caveat_widening(), explicit_shell_deny_precedes_uncovered_egress_class(), kernel_origin_cannot_bypass_rated_egress_class(), KernelTrustedRated, expired_invalid_mac_is_caveat_widening_not_grant_expired(), gate_denies_catalog_unknown_id_with_unknown_action_reason(), gate_denies_stale_granted_but_catalog_unknown_id(), gate_keeps_not_granted_for_known_ungranted_id() (+59 more)

### Community 3 - "handle_owner_update"
Cohesion: 0.07
Nodes (29): DeserializeOwned, complete_u32(), divergent_inputs_fail_closed(), ledger_corruption_fails_closed(), ModelOutput, (), ApprovalStepInputs, ArtifactRef (+21 more)

### Community 4 - ".default"
Cohesion: 0.18
Nodes (8): canonical_catalog_covers_all_fixture_action_ids(), kind_table_excludes_templates(), kind_table_round_trips_all_six_kinds(), loads_every_real_fixture_without_error(), malformed_fixture_fails_to_load(), missing_directory_is_not_an_error(), non_yaml_files_are_ignored(), repo_lyra_dir()

### Community 5 - "GmailConnector"
Cohesion: 0.08
Nodes (23): counterparty_resolves_identity_but_no_principal(), handle_owner_bind(), IdentityResolver, IdentityResolver<'a>, owner_verified_path_resolves_owner_principal_and_relationship(), unknown_resolves_to_relationship_unknown_confidence_0_and_no_write(), is_unique_constraint_store_error(), is_unique_constraint_violation() (+15 more)

### Community 6 - "artifact_loader.rs"
Cohesion: 0.14
Nodes (26): connector_counters_record_success_and_failure(), connector_failure_batches_and_is_audited(), counter_persistence_failure_is_durably_batched_as_resource(), immediate_failure_cannot_enter_digest_lane(), test_state_with_telegram(), batch_failure(), encrypt_digest_detail(), FailureClass (+18 more)

### Community 7 - "config.rs"
Cohesion: 0.15
Nodes (34): handle_owner_update(), owner_update(), audit_append_failure_fails_notification_before_connector_effect(), audit_readonly_failure_fails_notification_before_connector_effect(), disk_full_audit_append_aborts_before_connector_effect(), audit_payload_refs(), draft_command_composes_email_preview_grant_whose_pending_ref_is_derived_message(), draft_command_for_a_missing_thread_mints_no_grant() (+26 more)

### Community 8 - "mod.rs"
Cohesion: 0.11
Nodes (32): test_state(), concurrent_owner_callbacks_are_serialized_by_handler(), notify_owner_required_errors_and_audits_on_send_failure(), notify_owner_required_succeeds_when_sent(), notify_owner_with_digest_records_success_counter(), notify_send_failure_records_attempt_failure_and_dead_letter(), parse_audit_event(), required_send_failure_keeps_dlq_when_counter_persistence_breaks() (+24 more)

### Community 9 - "telegram.rs"
Cohesion: 0.05
Nodes (42): Purpose, Requirement: ActionCatalog MUST enumerate every trusted-path carve-out around gate(), Requirement: Approval-required decisions MUST not execute immediately, Requirement: Every effectful action MUST pass through gate(), Requirement: Gate decisions MUST be auditable, Requirement: Gate decisions MUST use task grant precedence, Requirement: Gate MUST verify authenticated grant caveat chains offline, Requirement: Grant limits MUST be enforced at runtime (+34 more)

### Community 10 - ".put"
Cohesion: 0.12
Nodes (36): artifact_identity_pairs(), artifact_version(), ArtifactKindSpec, ArtifactLoadError, ArtifactRegistry, ArtifactSource, exclude_identity_pairs(), find_kind_spec() (+28 more)

### Community 11 - "ProposedArtifact"
Cohesion: 0.15
Nodes (22): counterparty_deferral_text_is_canonical(), grant_with_thread(), no_thread_id_resolves_to_master(), owner_escalation_message(), owner_message_carries_action_and_reason_code(), resolve_by_thread_id_returns_bound_grant(), resolve_grant_for_thread(), route_escalation() (+14 more)

### Community 12 - "client.rs"
Cohesion: 0.14
Nodes (15): Arc, ArtifactRef, AtomicBool, Connection, Error, Mutex, Option, Path (+7 more)

### Community 13 - "policy.rs"
Cohesion: 0.53
Nodes (5): plan_preview_records_telegram_success_counter(), plan_proposal_budget_exhaustion_persists_no_request(), plan_propose_approve_rederives_gate_and_resolves(), proposed_plan_fixture(), tampered_plan_artifact_is_refused_at_approval_callback()

### Community 14 - "actions.rs"
Cohesion: 0.07
Nodes (50): BuildEnvelopeFn, Future, GrantBindingFn, Output, Pin, PreflightFn, RouteGuardFn, Send (+42 more)

### Community 15 - "ADDED Requirements"
Cohesion: 0.05
Nodes (37): Purpose, Requirement: A mutated approved plan MUST be refused at the gate, Requirement: Approval audit MUST avoid plaintext private payloads, Requirement: Approval MUST bind only what the owner was shown, Requirement: Approvals MUST expire, Requirement: Draft creation MUST remain approval-required, Requirement: Draft creation MUST require digest-bound approval, Requirement: Final email send MUST remain denied (+29 more)

### Community 16 - "Requirements"
Cohesion: 0.06
Nodes (34): Purpose, Requirement: Action requests and gate decisions MUST be typed, Requirement: Approval records MUST bind reviewed payloads and targets, Requirement: Audit schemas MUST reference private payloads by encrypted or hash refs, Requirement: Event bus subscription types MUST be explicit schemas, Requirement: Event envelopes MUST include source authenticity fields, Requirement: EventEnvelope MUST carry an optional dormant thread_id, Requirement: Identity schemas MUST NOT grant runtime authority (+26 more)

### Community 17 - "content.d.ts"
Cohesion: 0.10
Nodes (35): anthropic_client_parses_the_reply_text(), GatewayError, generate_anthropic(), generate_openai_compat(), http_client(), malformed_response_is_missing_content_not_a_panic(), messages_json(), openai_compat_client_parses_the_reply_text() (+27 more)

### Community 19 - "action.rs"
Cohesion: 0.20
Nodes (22): initialization_transaction_failure_rolls_back_then_retry_succeeds(), mount_unused_provider(), provider_pointed_at(), startup_migrates_old_token_and_legacy_offset_before_first_poll(), startup_preserves_offset_when_vault_token_matches_persisted(), startup_reconciles_vault_token_when_persisted_bot_id_differs(), startup_retries_getme_on_transient_failure(), assert_poll_offset() (+14 more)

### Community 20 - "StoreError"
Cohesion: 0.08
Nodes (74): artifact_ref(), email_event(), email_reply_drafter_agent(), email_route(), empty_session_policy(), global_policy(), main_assistant_agent(), owner_control_basic_pack() (+66 more)

### Community 21 - "post_action"
Cohesion: 0.07
Nodes (27): ADDED Requirements, MODIFIED Requirements, Requirement: ActionCatalog MUST enumerate every trusted-path carve-out around gate(), Requirement: Approval-required decisions MUST not execute immediately, Requirement: Every effectful action MUST pass through gate(), Requirement: Gate decisions MUST be auditable, Requirement: Grant limits MUST be enforced at runtime, Requirement: Kernel-origin actions MUST route through gate() with a KernelOrigin marker (+19 more)

### Community 22 - "ApprovalRecord"
Cohesion: 0.07
Nodes (26): artifact-lifecycle Specification, Purpose, Requirement: Activated artifacts MUST survive a kernel restart, Requirement: Activation MUST require digest-bound owner approval, Requirement: Artifact id and version MUST be unique across fixtures, overlay, and pending proposals, Requirement: Authority-bearing proposals require overlay evaluation before approval, Requirement: Only active artifacts MUST participate in authority composition, Requirement: Prompt templates MUST NOT be proposable at runtime (+18 more)

### Community 23 - "ActionId"
Cohesion: 0.20
Nodes (24): accepted_digest_tamper_invalidates(), base_namespace_is_not_treated_as_learned(), dangling_learned_route_is_orphaned_and_excluded(), insert_agent(), insert_pack(), insert_route(), insert_workflow(), learned() (+16 more)

### Community 24 - "AppState"
Cohesion: 0.12
Nodes (26): ActivationCommit, ArtifactRef, Option, Result, Ulid, Store, CompatibilityStatus, dependency_fingerprint_allows() (+18 more)

### Community 25 - "AppState"
Cohesion: 0.11
Nodes (13): canonical_catalog(), counterparty_classification_is_kernel_owned_and_fails_closed(), id(), test_catalog_effect_paths_are_fully_enumerated_and_classified(), From, action_id_qualifier_is_part_of_identity(), action_id_serializes_as_bare_string(), ActionCatalog (+5 more)

### Community 26 - ".sweep_expired_grants"
Cohesion: 0.08
Nodes (25): ADDED Requirements, Requirement: Authority-sensitive changes MUST be explicitly marked, Requirement: Decision-log consistency MUST be preserved, Requirement: Every change MUST classify its affected layer, Requirement: OpenSpec archive MUST preserve rationale, Requirement: OpenSpec development process MUST define its purpose, Requirement: OpenSpec MUST remain separate from OpenSpine runtime authority, Requirement: PRD-derived work MUST be split into implementation slices (+17 more)

### Community 27 - "Lifecycle"
Cohesion: 0.18
Nodes (14): build_raw_reply_message(), CachedToken, extract_body_text(), extract_email_address(), GmailConnector, GmailError, GmailMessage, GmailThread (+6 more)

### Community 28 - "Requirements"
Cohesion: 0.13
Nodes (17): EvalRow, Row, TryFrom, digest(), fractional_timestamp_orders_after_exact_second(), insert_and_query_by_artifact_returns_ordered_rows(), latest_eval_verdict_returns_newest_for_artifact(), query_by_verdict_filters_across_artifacts() (+9 more)

### Community 29 - "OpenSpine Agent-OS Design Log"
Cohesion: 0.05
Nodes (22): AsRef, ConsumerError, IdempotentConsumer, LedgerEntry, PersistedConsumerState, Store, AuditEvent, AuditKind (+14 more)

### Community 30 - "ArtifactRef"
Cohesion: 0.08
Nodes (25): Purpose, Requirement: Authority-sensitive changes MUST be explicitly marked, Requirement: Completed OpenSpec changes MUST be archived, Requirement: Decision-log consistency MUST be preserved, Requirement: Each OpenSpec change MUST state affected layer, Requirement: OpenSpec artifacts MUST NOT be treated as live runtime authority, Requirement: OpenSpec development process MUST define its purpose, Requirement: PRD-derived work MUST be split into implementation slices (+17 more)

### Community 31 - "ADDED Requirements"
Cohesion: 0.08
Nodes (23): day-2-operations Specification, Purpose, Requirement: Audit I/O failure handling, Requirement: Boot clock-regression detection, Requirement: One-set snapshot and restore, Requirement: Same-conversation serialization, Requirement: Telegram-first first-run sequence, Requirement: Versioned schema migrations (+15 more)

### Community 32 - "Requirements"
Cohesion: 0.14
Nodes (10): evaluate(), JudgeDenial, GateDenial, GateEvidence, JudgePassed, ReplayPassed, run_gate(), run_model_swap_gate() (+2 more)

### Community 35 - "sandbox.rs"
Cohesion: 0.08
Nodes (24): model-swap-ceremony Specification, Purpose, Requirement: Activation MUST use a serialized provenance-bound staged protocol, Requirement: Golden sets MUST be bounded and role-bound, Requirement: Model swaps MUST be evidence-bearing AD-142 proposals, Requirement: Provider configuration changes MUST NOT bypass the ceremony, Requirement: Restart MUST require symmetric latest ceremony provenance, Requirements (+16 more)

### Community 38 - "ADDED Requirements"
Cohesion: 0.19
Nodes (17): ActionBody, ActionOutcome, approval_required_is_ok_not_err(), counterparty_denial_preserves_only_canonical_deferral(), deny_decision_is_ok_not_err(), generate_sends_bearer_auth(), generate_sends_untrusted_context_in_body(), GenerateBody (+9 more)

### Community 39 - "Requirements"
Cohesion: 0.09
Nodes (12): Store, DeadLetterState, detail_insert_columns(), DetailReceipt, DigestItem, NotifyDeadLetter, parse_availability_outcome(), unavailable_outcome() (+4 more)

### Community 40 - "Requirements"
Cohesion: 0.08
Nodes (24): Purpose, Requirement: Attachments MUST be denied in the preview slice, Requirement: Email content MUST be treated as untrusted data, Requirement: Email read MUST be selected-thread only, Requirement: Email workflow MUST require a trusted selected-thread token, Requirement: Model calls with private email context MUST use model gateway, Requirement: Preview output MUST be reviewable by the owner, Requirement: Preview slice MUST NOT send email (+16 more)

### Community 41 - "Requirements"
Cohesion: 0.08
Nodes (23): ADDED Requirements, Model swap ceremony Specification, Requirement: Activation MUST use a serialized provenance-bound staged protocol, Requirement: Golden sets MUST be bounded and role-bound, Requirement: Model swaps MUST be evidence-bearing AD-142 proposals, Requirement: Provider configuration changes MUST NOT bypass the ceremony, Requirement: Restart MUST require symmetric latest ceremony provenance, Scenario: Approved Base swap changes the gateway selection (+15 more)

### Community 42 - "Requirements"
Cohesion: 0.30
Nodes (15): test_state_with_gmail(), lyra_ui_preview_sends_telegram_reply_to_grant_bound_chat(), lyra_ui_preview_truncates_long_body_to_utf16_limit(), truncated_preview_carries_no_approval_button_and_persists_no_action_request(), email_read_selected_thread_rejects_expired_token(), email_read_selected_thread_rejects_foreign_grant(), email_read_selected_thread_rejects_malformed_payload(), email_read_selected_thread_rejects_second_use() (+7 more)

### Community 43 - "properties"
Cohesion: 0.16
Nodes (22): apply_compatibility(), compatibility_epoch(), dangling_for_parsed(), ensure_reconfirm_request(), exclude_orphans(), find_orphans(), missing_provenance(), OrphanedArtifact (+14 more)

### Community 44 - "ADDED Requirements"
Cohesion: 0.09
Nodes (22): ADDED Requirements, Artifact lifecycle overlay delta, MODIFIED Requirements, Requirement: Activated artifacts MUST survive a kernel restart, Requirement: Base and overlay namespace collisions MUST refuse replacement, Requirement: Compatibility MUST fail closed for dangling learned references, Requirement: Learned artifacts MUST carry durable exchange provenance, Requirement: Legacy overlay migration is discovery/quarantine only (+14 more)

### Community 45 - "digest.rs"
Cohesion: 0.09
Nodes (22): Purpose, Requirement: Connector secrets MUST be captured outside shell/model context, Requirement: Connectors MUST resolve credentials at call time, Requirement: Intake and rotation mode transitions MUST use normal gate authority, Requirement: Paired Gmail credentials MUST be staged and promoted atomically, Requirement: Pending captures MUST be bound and fail closed, Requirement: Secret intake outcomes MUST be metadata-only and auditable, Requirement: Secret values MUST be encrypted at rest and rotatable (+14 more)

### Community 46 - "Digest"
Cohesion: 0.09
Nodes (22): Adversarial review (from AdversarialAgentLit research, 2026-07-07), Authority growth (settled), Base/overlay & updates (settled), Blindspot resolutions (2026-07-07, owner-approved: recommendations Q1-Q7 adopted), Core axes (settled), Delegation & containment (settled), Egress & connectors (settled), Game-AI patterns (from GameAiPatterns research, 2026-07-07) (+14 more)

### Community 47 - "Design: OpenSpine development process"
Cohesion: 0.09
Nodes (21): ADDED Requirements, Requirement: Action requests and gate decisions MUST be typed, Requirement: Approval records MUST bind reviewed payloads and targets, Requirement: Audit schemas MUST reference private payloads by encrypted or hash refs, Requirement: Event envelopes MUST include source authenticity fields, Requirement: Identity schemas MUST NOT grant runtime authority, Requirement: OpenSpine core runtime objects MUST have explicit schemas, Requirement: Route resolution schemas MUST represent ambiguity (+13 more)

### Community 48 - "ADDED Requirements"
Cohesion: 0.09
Nodes (21): ADDED Requirements, Requirement: Connector secrets MUST be captured outside shell/model context, Requirement: Connectors MUST resolve credentials at call time, Requirement: Intake and rotation mode transitions MUST use normal gate authority, Requirement: Paired Gmail credentials MUST be staged and promoted atomically, Requirement: Pending captures MUST be bound and fail closed, Requirement: Secret intake outcomes MUST be metadata-only and auditable, Requirement: Secret values MUST be encrypted at rest and rotatable (+13 more)

### Community 49 - "ADDED Requirements"
Cohesion: 0.16
Nodes (24): AppState, dispatch_polled_updates(), kernel_notify_grant(), mint_reconfirm_grant(), notify_owner_required(), notify_owner_required_outcome(), notify_owner_with_digest(), NotifyOutcome (+16 more)

### Community 50 - "mod.rs"
Cohesion: 0.10
Nodes (20): ADDED Requirements, Purpose, Requirement: Allowed-action dispatch MUST resolve through a handler registry, Requirement: Connectors MUST be registered through a connector registry, Requirement: Post-approval resolution MUST route through a registry with a draft-creation default, Requirement: Proposable artifact kinds MUST have a single source of truth, Requirement: Unknown action ids MUST be denied at gate with a structured reason, Requirement: Unknown action ids MUST fail fast at composition (+12 more)

### Community 51 - "ADDED Requirements"
Cohesion: 0.10
Nodes (20): failure-surfacing Specification, Purpose, Requirement: Artifact-backed dead letters, Requirement: Connector counters, Requirement: Delivery-unknown crash semantics, Requirement: Direct authenticated bad-request surfacing, Requirement: Durable effect receipts, Requirement: Failure taxonomy routing (+12 more)

### Community 52 - "ADDED Requirements"
Cohesion: 0.10
Nodes (20): kernel-registries Specification, Purpose, Requirement: Allowed-action dispatch MUST resolve through a handler registry, Requirement: Connectors MUST be registered through a connector registry, Requirement: Post-approval resolution MUST route through a registry with a draft-creation default, Requirement: Proposable artifact kinds MUST have a single source of truth, Requirement: Unknown action ids MUST be denied at gate with a structured reason, Requirement: Unknown action ids MUST fail fast at composition (+12 more)

### Community 53 - "Requirements"
Cohesion: 0.13
Nodes (34): activation_with_mutated_payload_is_denied(), approve_callback_update(), approved_artifact_activates_into_registry_and_overlay(), model_swap_ceremony_switches_real_generate_provider(), mount_send_message_ok(), MockServer, TelegramConnector, Ulid (+26 more)

### Community 54 - "Requirements"
Cohesion: 0.18
Nodes (16): AgentLimits, AgentManifest, main_assistant_agent(), main_assistant_denies_broad_email_access(), MemoryScope, ModelPolicy, OutputChannels, Persistence (+8 more)

### Community 55 - "Requirements"
Cohesion: 0.10
Nodes (19): ADDED Requirements, Purpose, Requirement: Activated artifacts MUST survive a kernel restart, Requirement: Activation MUST require digest-bound owner approval, Requirement: Artifact id and version MUST be unique across fixtures, overlay, and pending proposals, Requirement: Only active artifacts MUST participate in authority composition, Requirement: Prompt templates MUST NOT be proposable at runtime, Requirement: Proposed artifacts MUST be schema-validated before persistence (+11 more)

### Community 56 - "owner_event"
Cohesion: 0.10
Nodes (19): ADDED Requirements, Failure surfacing, Requirement: Artifact-backed dead letters, Requirement: Connector counters, Requirement: Delivery-unknown crash semantics, Requirement: Direct authenticated bad-request surfacing, Requirement: Durable effect receipts, Requirement: Failure taxonomy routing (+11 more)

### Community 57 - ".default"
Cohesion: 0.10
Nodes (19): Purpose, Requirement: Approval-required MUST override plain allow, Requirement: Authority composition MUST be deny-by-default, Requirement: Authority widening MUST require explicit approval, Requirement: Connector and account role MUST NOT grant authority by themselves, Requirement: Explicit deny MUST override allow, Requirement: Identity MUST NOT grant authority by itself, Requirement: Main assistant grant MUST NOT inherit specialist workflow authority (+11 more)

### Community 58 - "Runtime schema groups"
Cohesion: 0.15
Nodes (18): truncate_for_telegram(), truncate_with_notice(), ActionRequestBody, ActionResponseBody, dispatch_allowed_action(), dispatch_lyra_preview(), dispatch_read_selected_thread(), DispatchError (+10 more)

### Community 59 - "ADDED Requirements"
Cohesion: 0.19
Nodes (5): secret_introduction_and_rotation_are_decryptable(), SecretStore, SecretStoreError, seed_is_idempotent_and_slot_validation_is_strict(), truncated_or_corrupted_ciphertext_fails_closed()

### Community 60 - "ADDED Requirements"
Cohesion: 0.13
Nodes (8): AgentManifest, CapabilityPack, ModelSwapManifest, ParsedProposal, Policy, PromptTemplate, Versioned, WorkflowManifest

### Community 61 - "ADDED Requirements"
Cohesion: 0.27
Nodes (17): allow_reply(), allow_result(), approval_required_on_primary_action_exits_ok_no_reply(), cmd_freeform(), cmd_propose(), cmd_setup(), cmd_status(), deny_on_model_generate_exits_ok() (+9 more)

### Community 62 - "ADDED Requirements"
Cohesion: 0.11
Nodes (18): ADDED Requirements, Requirement: Approval-required MUST override plain allow, Requirement: Authority composition MUST be deny-by-default, Requirement: Authority widening MUST require explicit approval, Requirement: Connector and account role MUST NOT grant authority by themselves, Requirement: Explicit deny MUST override allow, Requirement: Identity MUST NOT grant authority by itself, Requirement: Main assistant grant MUST NOT inherit specialist workflow authority (+10 more)

### Community 63 - "Requirements"
Cohesion: 0.11
Nodes (18): ADDED Requirements, MODIFIED Requirements, Requirement: Approval audit MUST avoid plaintext private payloads, Requirement: Approval MUST bind only what the owner was shown, Requirement: Draft creation MUST require digest-bound approval, Requirement: The kernel MUST re-derive digests from artifact-store bytes at approval-effect time, Scenario: Approval is recorded, Scenario: Audit records the re-derived digest (+10 more)

### Community 64 - "scripts"
Cohesion: 0.11
Nodes (18): ADDED Requirements, MODIFIED Requirements, Requirement: A mutated approved plan MUST be refused at the gate, Requirement: Plan approval MUST bind the complete ordered step-list digest, Requirement: Plan steps MUST bind exact execution identity, Requirement: The kernel MUST re-derive digests from artifact-store bytes at approval-effect time, Scenario: Arguments change while summary remains unchanged, Scenario: Data-handling step participates in the digest (+10 more)

### Community 65 - "compose_authority"
Cohesion: 0.11
Nodes (18): audit-artifact-store Specification, Purpose, Requirement: Artifact blobs MUST be encrypted and content-addressed by plaintext digest, Requirement: Audit append MUST assign per-aggregate sequence under the store lock, Requirement: Reading an artifact MUST re-verify its digest after decryption, Requirement: Task tokens MUST be stored hashed, never in plaintext, Requirement: The audit log MUST be append-only and hash-chained, Requirement: The kernel MUST verify the audit chain on startup and refuse to start if broken (+10 more)

### Community 66 - "ConnectorRegistry"
Cohesion: 0.11
Nodes (17): 1. `define-core-runtime-schemas`, 2. `implement-authority-composition`, 3. `implement-gate-action-api`, 4. `implement-telegram-owner-control-slice`, 5. `implement-selected-thread-email-preview-slice`, Authority-sensitive changes, Context, Design goals (+9 more)

### Community 67 - "identity.rs"
Cohesion: 0.09
Nodes (22): ADDED Requirements, Day-2 Operations, Requirement: Audit I/O failure handling, Requirement: Boot clock-regression detection, Requirement: One-set snapshot and restore, Requirement: Same-conversation serialization, Requirement: Telegram-first first-run sequence, Requirement: Versioned schema migrations (+14 more)

### Community 68 - "properties"
Cohesion: 0.07
Nodes (27): Cli, commit_post_bind_clock(), grant_hmac_key(), main(), Option, PathBuf, Result, Store (+19 more)

### Community 69 - "tests.rs"
Cohesion: 0.14
Nodes (12): Sha256, canonical_json(), CanonicalValue, Digest, digest_from_hash(), digest_of(), digest_of_bytes_hashes_raw_content_directly(), digest_of_is_a_pinned_golden_value() (+4 more)

### Community 70 - "Proposal: Define OpenSpine development process"
Cohesion: 0.11
Nodes (17): ADDED Requirements, Requirement: Attachments MUST be denied in the preview slice, Requirement: Email content MUST be treated as untrusted data, Requirement: Email read MUST be selected-thread only, Requirement: Email workflow MUST require a trusted selected-thread token, Requirement: Model calls with private email context MUST use model gateway, Requirement: Preview output MUST be reviewable by the owner, Requirement: Preview slice MUST NOT send email (+9 more)

### Community 71 - "MODIFIED Requirements"
Cohesion: 0.11
Nodes (17): escalation-and-refusal Specification, Purpose, Requirement: Counterparty-facing gate denials at the worker action chokepoint MUST surface only the canonical deferral plus an owner-routed escalation, Requirement: Escalation routing MUST be deterministic kernel machinery, Requirement: No policy or rule text MUST cross the worker-facing chokepoint as human-facing content, Requirement: Thread↔grant binding MUST be kernel-owned and dormant until a thread-capable channel ships, Requirements, Scenario: Allowed action does not escalate or defer (+9 more)

### Community 72 - "properties"
Cohesion: 0.12
Nodes (16): ADDED Requirements, Purpose, Requirement: Lane specifications MUST be compiled-in kernel data, Requirement: Per-flow variation MUST be lane data interpreted by one driver, Requirement: The audited event envelope MUST be emitted only after verification succeeds, Requirement: The driver MUST NOT invoke gate(), Requirement: The kernel pipeline MUST be a typed stage sequence the driver executes, Scenario: A lane cannot skip a stage (+8 more)

### Community 73 - "create_approved_draft"
Cohesion: 0.24
Nodes (3): genesis_digest(), digest_matches_hash(), Store

### Community 74 - ".put"
Cohesion: 0.12
Nodes (16): ADDED Requirements, Requirement: Counterparty-facing gate denials at the worker action chokepoint MUST surface only the canonical deferral plus an owner-routed escalation, Requirement: Escalation routing MUST be deterministic kernel machinery, Requirement: No policy or rule text MUST cross the worker-facing chokepoint as human-facing content, Requirement: Thread↔grant binding MUST be kernel-owned and dormant until a thread-capable channel ships, Scenario: Allowed action does not escalate or defer, Scenario: Counterparty ApprovalRequired also returns deferral, routes to owner, and audits, Scenario: Counterparty deferral is exactly the canonical constant (+8 more)

### Community 75 - "SKILL.md"
Cohesion: 0.11
Nodes (19): Option, Ulid, Vec, Store, ApprovalDecision, ApprovalRecord, matches_rejects_expired_approval(), matches_rejects_non_approved_decisions() (+11 more)

### Community 76 - "Tasks: Harden approval and budgets"
Cohesion: 0.12
Nodes (16): pipeline-driver Specification, Purpose, Requirement: Lane specifications MUST be compiled-in kernel data, Requirement: Per-flow variation MUST be lane data interpreted by one driver, Requirement: The audited event envelope MUST be emitted only after verification succeeds, Requirement: The driver MUST NOT invoke gate(), Requirement: The kernel pipeline MUST be a typed stage sequence the driver executes, Requirements (+8 more)

### Community 77 - "properties"
Cohesion: 0.29
Nodes (15): denied_read_thread_stops_without_drafting(), Draft, draft_reply(), empty_draft_skips_preview_without_error(), format_thread_for_model(), format_thread_for_model_includes_all_fields(), full_flow_reads_drafts_and_previews(), no_selection_tokens_is_an_error() (+7 more)

### Community 78 - "explore.md"
Cohesion: 0.12
Nodes (15): ADDED Requirements, Durable workflow replay, Ratified decisions, Requirement: Failures are terminal and replayable, Requirement: Gated and approval identity is bound, Requirement: Private payloads do not leak, Requirement: Timers have atomic durable transitions, Requirement: Workflow steps replay by exact durable handle (+7 more)

### Community 79 - "OpenSpine kernel↔shell HTTP contract"
Cohesion: 0.20
Nodes (3): LineageParent, root_has_generation_zero_and_no_parents(), root_round_trips_through_json()

### Community 80 - "Ok"
Cohesion: 0.12
Nodes (25): quarantine_pending_overlay(), reconcile_model_swap_overlay(), Path, Result, Store, discard_staged_overlay_files(), load(), OverlayStartup (+17 more)

### Community 81 - "Proposal: Define core runtime schemas"
Cohesion: 0.21
Nodes (21): build_state(), build_state_with_store(), repo_lyra_dir(), GmailConnector, Option, PathBuf, Store, TelegramConnector (+13 more)

### Community 82 - "Proposal: Implement authority composition"
Cohesion: 0.12
Nodes (15): durable-workflow-replay Specification, Purpose, Requirement: Failures are terminal and replayable, Requirement: Gated and approval identity is bound, Requirement: Private payloads do not leak, Requirement: Timers have atomic durable transitions, Requirement: Workflow steps replay by exact durable handle, Requirements (+7 more)

### Community 83 - "Proposal: Implement digest-bound draft approval"
Cohesion: 0.12
Nodes (15): lineage-and-eval-store Specification, Purpose, Requirement: Artifacts MUST carry a generation/lineage model distinct from content version, Requirement: Eval-verdict vocabulary MUST remain open and fitness/evidence optional, Requirement: Eval verdicts MUST land in an indexed table, not the audit chain, Requirement: Unknown lineage MUST NOT be rewritten as root, Requirements, Scenario: A row with no lineage loads as None (+7 more)

### Community 84 - "Proposal: Implement gate action API"
Cohesion: 0.29
Nodes (22): highest_active_prunes_stale_loaded_lower_version(), missing_highest_blob_fails_closed_no_rollback(), on_disk_artifact_without_active_proposal_is_excluded(), active_v2_survives_stale_approved_v1_recovery(), approved_dangling_recovery_quarantines_and_requests_reconfirm(), commit_before_publish_crash_republishes_exact_active_ref(), fixture(), insert_active_proposal() (+14 more)

### Community 85 - "Proposal: Implement selected-thread email preview slice"
Cohesion: 0.30
Nodes (4): Option, Result, Ulid, Store

### Community 86 - "Tasks: Implement selected-thread email preview slice"
Cohesion: 0.06
Nodes (40): GoldenSet, GoldenSetCase, GoldenSetCaseKind, GoldenSetCaseResult, GoldenSetRunResult, GoldenSetValidationError, ModelRole, ModelSwapManifest (+32 more)

### Community 87 - "Proposal: Implement Telegram owner control slice"
Cohesion: 0.13
Nodes (14): ADDED Requirements, Purpose, Requirement: Artifact blobs MUST be encrypted and content-addressed by plaintext digest, Requirement: Reading an artifact MUST re-verify its digest after decryption, Requirement: Task tokens MUST be stored hashed, never in plaintext, Requirement: The audit log MUST be append-only and hash-chained, Requirement: The kernel MUST verify the audit chain on startup and refuse to start if broken, Scenario: A row is appended (+6 more)

### Community 88 - "Proposal: Backfill implemented capability specs"
Cohesion: 0.13
Nodes (14): ADDED Requirements, Purpose, Requirement: The Docker driver MUST provide no-public-egress networking, a read-only rootfs, and a non-root user, Requirement: The kernel MUST refuse external-communication events under the Process driver without explicit opt-in, Requirement: The kernel↔shell transport trust assumption MUST be documented, Requirement: The shell environment MUST contain only KERNEL_ENDPOINT and TASK_TOKEN, Scenario: A shell container is spawned in production, Scenario: A shell container is spawned under DockerDriver (+6 more)

### Community 89 - "Proposal: Harden approval and budgets"
Cohesion: 0.13
Nodes (14): ADDED Requirements, lineage-and-eval-store Specification, Requirement: Artifacts MUST carry a generation/lineage model distinct from content version, Requirement: Eval-verdict vocabulary MUST remain open and fitness/evidence optional, Requirement: Eval verdicts MUST land in an indexed table, not the audit chain, Requirement: Unknown lineage MUST NOT be rewritten as root, Scenario: A row with no lineage loads as None, Scenario: An open-vocabulary verdict is accepted (+6 more)

### Community 90 - "Proposal: Implement artifact lifecycle slice"
Cohesion: 0.13
Nodes (14): Acceptance Criteria, Affected layer, Authority sensitivity, Decision-log check, Dependencies, Goals, Non-goals, Out of Scope (+6 more)

### Community 91 - "Tasks: Implement artifact lifecycle slice"
Cohesion: 0.13
Nodes (14): Acceptance Criteria, Affected layer, Authority sensitivity, Decision-log check, Dependencies, Goals, Non-goals, Out of Scope (+6 more)

### Community 92 - "Agent-OS change sequence (2026-07-07, AD canon)"
Cohesion: 0.13
Nodes (14): Purpose, Requirement: The Docker driver MUST provide no-public-egress networking, a read-only rootfs, and a non-root user, Requirement: The kernel MUST refuse external-communication events under the Process driver without explicit opt-in, Requirement: The kernel↔shell transport trust assumption MUST be documented, Requirement: The shell environment MUST contain only KERNEL_ENDPOINT and TASK_TOKEN, Requirements, Scenario: A shell container is spawned in production, Scenario: A shell container is spawned under DockerDriver (+6 more)

### Community 93 - "AuditMeta"
Cohesion: 0.13
Nodes (14): Purpose, Requirement: Main assistant task grant MUST be narrow, Requirement: Owner Telegram messages MUST normalize into event envelopes, Requirement: Telegram owner messages MUST be source verified, Requirement: Telegram owner route MUST resolve deterministically, Requirement: Telegram reply MUST use the owner channel only, Requirements, Scenario: Agent attempts reply to different chat (+6 more)

### Community 94 - "AuditEvent"
Cohesion: 0.32
Nodes (11): agent_manifests_round_trip(), artifacts_dir(), email_grant_pack_excludes_read_inbox_and_send(), every_fixture_file_is_covered_by_a_test(), global_policy_round_trips_and_denies_send(), owner_control_pack_round_trips(), owner_email_selected_thread_route_is_expressible_declaratively(), owner_telegram_route_is_expressible_declaratively() (+3 more)

### Community 95 - "Tasks: Define core runtime schemas"
Cohesion: 0.14
Nodes (13): Artifacts and audit, Authority, Connectors and model calls, Context, Decision, Design: Core runtime schemas, Event and authenticity, Execution boundary (+5 more)

### Community 96 - "Tasks: Define OpenSpine development process"
Cohesion: 0.14
Nodes (13): ADDED Requirements, Requirement: Approval audit MUST avoid plaintext private payloads, Requirement: Draft creation MUST remain approval-required, Requirement: Draft creation MUST require digest-bound approval, Requirement: Final email send MUST remain denied, Requirement: Target mutation MUST invalidate approval, Scenario: Agent requests send after draft creation, Scenario: Approval is recorded (+5 more)

### Community 97 - "Tasks: Implement digest-bound draft approval"
Cohesion: 0.14
Nodes (13): ADDED Requirements, Requirement: Approval-required decisions MUST not execute immediately, Requirement: Every effectful action MUST pass through gate(), Requirement: Gate decisions MUST be auditable, Requirement: Gate decisions MUST use task grant precedence, Requirement: Unspecified actions MUST be denied, Scenario: Action appears in allowed and approval-required lists, Scenario: Action appears in allowed and denied lists (+5 more)

### Community 98 - "Tasks: Implement Telegram owner control slice"
Cohesion: 0.14
Nodes (13): ADDED Requirements, Requirement: Main assistant task grant MUST be narrow, Requirement: Owner Telegram messages MUST normalize into event envelopes, Requirement: Telegram owner messages MUST be source verified, Requirement: Telegram owner route MUST resolve deterministically, Requirement: Telegram reply MUST use the owner channel only, Scenario: Agent attempts reply to different chat, Scenario: Configured owner sends message (+5 more)

### Community 99 - "artifact_activation_tests.rs"
Cohesion: 0.14
Nodes (13): ADDED Requirements, Purpose, Requirement: Conversation state MUST store only role and artifact digest, Requirement: Private-context model calls MUST be constructed kernel-side, Requirement: Prompt templates MUST come from the kernel registry, never from shell input, Requirement: Provider credentials MUST never reach the shell, Requirement: Untrusted context MUST be wrapped with a per-call randomised delimiter, Scenario: A conversation turn is persisted (+5 more)

### Community 100 - "Design: Authority composition"
Cohesion: 0.23
Nodes (19): deny_limit_exceeded(), GenerateRequestBody, GenerateResponseBody, post_model_generate(), template_id_for_agent(), a_spoofed_closing_marker_in_the_content_does_not_escape_the_boundary(), build_prompt(), build_prompt_carries_system_preamble_and_conversation_through() (+11 more)

### Community 101 - "Tasks: Backfill implemented capability specs"
Cohesion: 0.28
Nodes (12): admits_more_than_three_tiny_items(), detail_pages(), detail_pages_reconstruct_every_byte_on_utf8_boundaries(), drains_aggregate_across_pages_without_duplicates(), handle_command(), handle_detail_command(), item(), long_detail_is_bounded_in_summary_and_full_via_ref() (+4 more)

### Community 102 - "ADDED Requirements"
Cohesion: 0.14
Nodes (13): model-gateway Specification, Purpose, Requirement: Conversation state MUST store only role and artifact digest, Requirement: Private-context model calls MUST be constructed kernel-side, Requirement: Prompt templates MUST come from the kernel registry, never from shell input, Requirement: Provider credentials MUST never reach the shell, Requirement: Untrusted context MUST be wrapped with a per-call randomised delimiter, Requirements (+5 more)

### Community 103 - "ADDED Requirements"
Cohesion: 0.14
Nodes (13): dependencies, astro, @astrojs/starlight, sharp, name, scripts, astro, build (+5 more)

### Community 104 - "OpenSpine conventions"
Cohesion: 0.17
Nodes (15): action_for(), arm(), CaptureOutcome, kind_prefix(), owner_grant(), parse_command(), Pending, rollback_secret() (+7 more)

### Community 105 - "openspine-decision-log.md"
Cohesion: 0.29
Nodes (4): Ok, CanonicalValue<'a>, HasherWriter<'a>, Write

### Community 106 - "D-051 — The agent-OS canon (AD-001..153) is decomposed into a dependency-edged change sequence; the stale later-changes placeholders are superseded or subsumed"
Cohesion: 0.26
Nodes (16): ActionHandler, ActionHandlerRegistry, handle_artifact_nominate(), handle_artifact_propose(), handle_lyra_preview(), handle_plan_propose(), handle_read_selected_thread(), handle_setup_workflow_start() (+8 more)

### Community 107 - "OpenSpine"
Cohesion: 0.47
Nodes (5): learned_template_row(), orphan_active_yaml_without_db_row_is_quarantined(), overlay_template_yaml(), reconfirmed_legacy_template_stays_live_across_restart(), String

### Community 108 - "Gmail selected-thread email preview setup (Phase 2)"
Cohesion: 0.15
Nodes (12): 1. Extend `AuditEvent` — not a parallel envelope, 2. Schema: columns on `audit_log`, not a new events table, 3. Append path — sequence under the same lock as the insert, 4. Typed filter + ordered replay (read path), 5. Idempotent consumer — ack only after successful handling, 6. File layout (rebase-friendly), Alternatives considered, Approach (+4 more)

### Community 109 - "Telegram owner-control setup (Phase 1)"
Cohesion: 0.15
Nodes (12): artifact-lifecycle Specification Delta, MODIFIED Requirements, Requirement: Authority-bearing proposals require overlay evaluation before approval, Requirement: Proposed artifacts MUST be schema-validated before persistence, Scenario: An unknown kind is rejected, Scenario: Generic lifecycle bypass is rejected, Scenario: Malformed YAML is rejected, Scenario: Missing model-swap evaluation blocks approval (+4 more)

### Community 110 - "Tasks: Implement authority composition"
Cohesion: 0.15
Nodes (12): event-bus Specification, Purpose, Requirement: Bus events MUST carry unique IDs and per-aggregate sequence numbers, Requirement: Consumers MUST be idempotent and ack only after successful handling, Requirement: Consumers MUST subscribe via typed filters and ordered ledger replay, Requirement: The event bus MUST be the append-only audit ledger with no parallel store, Requirements, Scenario: Append is durable before consumer observation (+4 more)

### Community 111 - "Design: Digest-bound draft approval"
Cohesion: 0.15
Nodes (12): identity-store Specification, Purpose, Requirement: A Principal is a first-class, authority-free record and v1 enforces exactly one owner, Requirement: Identity binding MUST happen only via an audited, owner-approved path, Requirement: Identity resolution MUST be a read-only seam that never binds or mints principals, Requirements, Scenario: Binding attempt without owner context is rejected, Scenario: Idempotent bootstrap establishes exactly one owner (+4 more)

### Community 112 - "Design: Gate action API"
Cohesion: 0.32
Nodes (11): activate_overlay_yaml(), prune_non_highest_active(), remove_loaded_version(), republish_missing_committed(), HashSet, Path, Result, Store (+3 more)

### Community 113 - "Tasks: Implement gate action API"
Cohesion: 0.17
Nodes (11): Goals, Non-goals, OpenSpec / OpenSpine boundary, OpenSpine / Lyra boundary, Proposal: Define OpenSpine development process, Proposed first implementation slices after this change, Risks, Scope (+3 more)

### Community 114 - "Design: Selected-thread email preview slice"
Cohesion: 0.17
Nodes (11): ADDED Requirements, MODIFIED Requirements, Requirement: Completed OpenSpec changes MUST be archived, Requirement: OpenSpec artifacts MUST NOT be treated as live runtime authority, Requirement: Security-load-bearing subsystems MUST gain a capability spec in the change that implements them, Scenario: A change implements a new gated subsystem, Scenario: Change is complete, Scenario: Completed process change (+3 more)

### Community 115 - "Design: Telegram owner control slice"
Cohesion: 0.17
Nodes (11): 1. Carve-out enumeration as data (D-055.1), 2. KernelOrigin marker (D-055.2), 3. Selection-token validation into gate() (D-055.3), 4. Digest re-derivation (D-055.4), Alternatives considered, Approach, Design: Harden gate trusted paths, Key decisions (D-055) (+3 more)

### Community 116 - "Requirement: Selection tokens MUST be single-use, expiring, and scope-fixed"
Cohesion: 0.17
Nodes (11): MODIFIED Requirements, Requirement: Email workflow MUST require a trusted selected-thread token, Requirement: Selection tokens MUST be single-use, expiring, and scope-fixed, Scenario: Gate denies a missing or wrong-type token, Scenario: Gate validates the selection token for the read action, Scenario: Shell provides thread ID directly, Scenario: Token reused after consumption, Scenario: Token used after expiry (+3 more)

### Community 117 - "D-063 — Model-swap activation uses a serialized staged recovery protocol"
Cohesion: 0.17
Nodes (11): 1. Pure surface types — schemas/escalation.rs, 2. Integrated chokepoint — POST /v1/actions denial branch, 3. Thread_id fields — EventEnvelope + TaskGrant, 4. Thread↔grant binding resolver — kernel/escalation.rs, 5. Mandatory owner delivery from the API layer, Alternatives considered, Approach, Authority sensitivity (+3 more)

### Community 118 - "Kernel foundation"
Cohesion: 0.17
Nodes (11): ADDED Requirements, Requirement: Bus events MUST carry unique IDs and per-aggregate sequence numbers, Requirement: Consumers MUST be idempotent and ack only after successful handling, Requirement: Consumers MUST subscribe via typed filters and ordered ledger replay, Requirement: The event bus MUST be the append-only audit ledger with no parallel store, Scenario: Append is durable before consumer observation, Scenario: Double filtered replay is a pure no-op, Scenario: Failed handling does not advance the checkpoint (+3 more)

### Community 119 - "attrs"
Cohesion: 0.17
Nodes (11): ADDED Requirements, Requirement: A Principal is a first-class, authority-free record and v1 enforces exactly one owner, Requirement: Identity binding MUST happen only via an audited, owner-approved path, Requirement: Identity resolution MUST be a read-only seam that never binds or mints principals, Scenario: Binding attempt without owner context is rejected, Scenario: Idempotent bootstrap establishes exactly one owner, Scenario: Owner asserts a binding successfully, Scenario: Owner resolves successfully (+3 more)

### Community 120 - "Design: Harden approval and budgets"
Cohesion: 0.17
Nodes (11): egress-classes Specification, Purpose, Requirement: Capability packs MUST reference allowed egress classes, Requirement: The connector registry MUST type and protect egress endpoints, Requirement: The gate MUST enforce registry-rated egress classes, Requirements, Scenario: A conflicting registration cannot downgrade an endpoint, Scenario: Pack egress classes become live grant authority (+3 more)

### Community 121 - "Failure surfacing & operations"
Cohesion: 0.18
Nodes (6): Identified, AgentManifest, CapabilityPack, Policy, PromptTemplate, WorkflowManifest

### Community 122 - "D-008 — Deterministic routing decides authority; agentic routing decides strategy"
Cohesion: 0.18
Nodes (10): Check for context, Ending Discovery, Guardrails, Handling Different Entry Points, OpenSpec Awareness, The Stance, What You Don't Have To Do, What You Might Do (+2 more)

### Community 123 - "D-001 — Lyra is a runtime/substrate, not a single agent"
Cohesion: 0.18
Nodes (10): Check for context, Ending Discovery, Guardrails, Handling Different Entry Points, OpenSpec Awareness, The Stance, What You Don't Have To Do, What You Might Do (+2 more)

### Community 124 - "mod.rs"
Cohesion: 0.19
Nodes (18): authenticate(), bearer_token(), get_status(), internal_error(), router(), Arc, ArtifactRef, Display (+10 more)

### Community 125 - "D-042 — Reply recipient is kernel-derived, never shell-supplied: newest non-owner sender, matched against a configured mailbox address"
Cohesion: 0.18
Nodes (6): Notes, Threat claims register, Canon sources, Completed / archived, OpenSpine OpenSpec change sequence, Reconciliation of the previous "later changes" list

### Community 126 - "D-002 — First usable UX should include an owner control channel"
Cohesion: 0.18
Nodes (10): Check for context, Ending Discovery, Guardrails, Handling Different Entry Points, OpenSpec Awareness, The Stance, What You Don't Have To Do, What You Might Do (+2 more)

### Community 127 - "D-003 — Gmail is a guarded workflow, not the whole product"
Cohesion: 0.18
Nodes (10): Check for context, Ending Discovery, Guardrails, Handling Different Entry Points, OpenSpec Awareness, The Stance, What You Don't Have To Do, What You Might Do (+2 more)

### Community 128 - "D-004 — Every effectful action goes through `gate()`"
Cohesion: 0.18
Nodes (10): Command, docker_driver_args_are_correct_and_secret_free(), DockerDriver, process_driver_allows_external_communication_with_explicit_opt_in(), process_driver_clears_env_and_sets_only_two_vars(), process_driver_never_refuses_owner_control_lane(), process_driver_refuses_external_communication_without_opt_in(), ProcessDriver (+2 more)

### Community 129 - "D-005 — Private-data shell must be contained"
Cohesion: 0.18
Nodes (10): 1. WYSIWYS (2a), 2. Enforce `max_model_calls` (2b), 3. Enforce `max_artifacts` (2c), 4. Hash task tokens at rest; sweep expired grants (2d), 5. Audit the trusted notification path (2e), 6. Operator docs (2f), 7. Spec deltas (2g), 8. Decision log (2h) (+2 more)

### Community 130 - "D-009 — External content is data, not instruction"
Cohesion: 0.18
Nodes (10): ADDED Requirements, Requirement: Gate MUST verify authenticated grant caveat chains offline, Requirement: Shadow-mode grants MUST return a non-executable decision, Scenario: Action outside an action_allowlist caveat is not granted, Scenario: Dispatch does not execute on effect_suppressed, Scenario: Shadow allow becomes effect_suppressed, Scenario: Shadow deny remains deny, Scenario: Tampered or reordered caveats are rejected (+2 more)

### Community 131 - "D-021 — Email domain is broader than Gmail"
Cohesion: 0.18
Nodes (10): 1. gate crate — pure `gate()` changes, 2. ActionCatalog metadata, 3. Kernel wiring, 4. Digest re-derivation at approval-effect time, 5. Characterization tests — one per enumerated carve-out entry, 6. Threat-claims register rows (implementation task — NOT edited here), 7. Decision-log D-055 (implementation task — NOT edited here), 8. Docs (+2 more)

### Community 132 - "D-022 — Agent-owned inbox is distinct from owner mailbox access"
Cohesion: 0.18
Nodes (10): ADDED Requirements, Egress classes Specification Delta, Requirement: Capability packs MUST reference allowed egress classes, Requirement: The connector registry MUST type and protect egress endpoints, Requirement: The gate MUST enforce registry-rated egress classes, Scenario: A conflicting registration cannot downgrade an endpoint, Scenario: Pack egress classes become live grant authority, Scenario: Registered web endpoints expose stable classes (+2 more)

### Community 133 - "D-023 — OpenSpine is the substrate; Lyra is a product built on it"
Cohesion: 0.18
Nodes (10): 1. Principal schema — authority-free, single-owner-shaped, 2. Identity store — DB-enforced single owner, audited binding, 3. IdentityResolver — read-only seam, owner fast path, 4. Composition cutover — principal_id, fail closed, 5. Owner-asserted binding — audited, owner-context-gated, agent-unreachable, Alternatives considered, Approach, Design: Implement identity store and principal (+2 more)

### Community 134 - "D-024 — OpenSpec is the development/change-management layer, not the runtime"
Cohesion: 0.38
Nodes (15): fixture(), mixed_model_swap_recovery_keeps_active_version_and_merge_preserves_it(), persist_active_provenance(), persist_proposal_state(), post_commit_pending_crash_restores_matching_reviewed_overlay(), pre_commit_pending_crash_discards_pending_and_keeps_old_disk(), Path, PathBuf (+7 more)

### Community 135 - "D-026 — Shell containment via a `SandboxDriver` trait (Process dev-only / Docker)"
Cohesion: 0.18
Nodes (10): Alternatives rejected, Approach, Authority and containment, Connector broker resolution, Design: Secret intake and rotation, Failure modes, Intake state machine, Paired Gmail staging state machine (+2 more)

### Community 136 - "banner"
Cohesion: 0.18
Nodes (15): ensure_schema(), lifecycle_name(), lineage_from_json(), lineage_to_json(), parse_lifecycle(), ProposedArtifact, Connection, Option (+7 more)

### Community 137 - "super::Store"
Cohesion: 0.13
Nodes (9): Connection, Path, Result, Store, String, Vec, super::Store, FnOnce (+1 more)

### Community 138 - "add_column_if_missing"
Cohesion: 0.31
Nodes (9): load_keyed(), load_registry_into(), HashMap, HashSet, Path, Result, String, T (+1 more)

### Community 139 - "Design: Backfill implemented capability specs"
Cohesion: 0.22
Nodes (8): OwnerReconfirmation, ArtifactRef, Option, Result, String, Ulid, Vec, Store

### Community 140 - "Design: Artifact lifecycle slice"
Cohesion: 0.20
Nodes (9): Check for context, Ending Discovery, Guardrails, OpenSpec Awareness, The Stance, What You Don't Have To Do, What You Might Do, When a change exists (+1 more)

### Community 141 - "Delegation & containment"
Cohesion: 0.20
Nodes (9): Authentication, Environment the shell process/container receives, Errors, `GET /v1/status`, `GET /v1/task`, OpenSpine kernel↔shell HTTP contract, `POST /v1/actions`, `POST /v1/model/generate` (+1 more)

### Community 142 - "Skills & workflows"
Cohesion: 0.20
Nodes (9): Check for context, Ending Discovery, Guardrails, OpenSpec Awareness, The Stance, What You Don't Have To Do, What You Might Do, When a change exists (+1 more)

### Community 143 - "Reflection & product surface"
Cohesion: 0.20
Nodes (9): Check for context, Ending Discovery, Guardrails, OpenSpec Awareness, The Stance, What You Don't Have To Do, What You Might Do, When a change exists (+1 more)

### Community 144 - "package.json"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Define core runtime schemas, Summary, What Changes (+1 more)

### Community 145 - "D-008 — Deterministic routing decides authority; agentic routing decides strategy"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Implement authority composition, Summary, What Changes (+1 more)

### Community 146 - "D-037 — Gmail OAuth via a plain refresh-token POST (no `oauth2` crate); `base64` promoted from transitive to direct dependency"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Implement digest-bound draft approval, Summary, What Changes (+1 more)

### Community 147 - "model_swap.rs"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Implement gate action API, Summary, What Changes (+1 more)

### Community 148 - "D-040 — Pending (pre-approval) `ActionRequest`s are persisted in a new `action_requests` table"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Implement selected-thread email preview slice, Summary, What Changes (+1 more)

### Community 149 - "D-041 — `email.create_draft`'s digest composition: payload = `{subject, body}`, target = `{thread_id, connector, account_role, recipients}`"
Cohesion: 0.20
Nodes (9): 1. Gmail connector skeleton, 2. Selection token, 3. Event and route, 4. Email read, 5. Model gateway, 6. Preview, 7. Tests, 8. Validation (+1 more)

### Community 150 - "D-043 — `lyra.ui.preview` is extended (not duplicated) to propose the exact reviewed draft and attach the approval button"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Implement Telegram owner control slice, Summary, What Changes (+1 more)

### Community 151 - "D-044 — Approved draft creation dispatches kernel-side; no new shell spawn"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Backfill implemented capability specs, Summary, What Changes (+1 more)

### Community 152 - "D-045 — WYSIWYS: a truncated preview refuses an approval button rather than splitting the message"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Harden approval and budgets, Summary, What Changes (+1 more)

### Community 153 - "D-046 — Grant budgets are enforced kernel-dispatch-side; the artifact budget counts only shell-initiated puts"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Implement artifact lifecycle slice, Summary, What Changes (+1 more)

### Community 154 - "D-047 — Task tokens are hashed at rest; expired grants are swept"
Cohesion: 0.20
Nodes (9): 1. Registry & schema plumbing, 2. Store, 3. Kernel: `artifact.propose`, 4. Kernel: approval branch + activation, 5. Fixtures + composition, 6. Shell: `/propose` UX, 7. Tests, 8. Validation (+1 more)

### Community 155 - "D-048 — `artifact.activate` is the single canonical activation action id; every runtime proposal requires uniform owner approval; prompt templates are excluded from proposable kinds"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Refactor kernel registries, Summary, What Changes (+1 more)

### Community 156 - "D-049 — Capability specs are backfilled for subsystems implemented inside earlier slices"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Refactor pipeline driver, Summary, What Changes (+1 more)

### Community 157 - "D-050 — `max_model_calls` is enforced with an atomic upsert, not a count-then-compare"
Cohesion: 0.20
Nodes (9): ADDED Requirements, Requirement: EventEnvelope MUST carry an optional dormant thread_id, Requirement: TaskGrant MUST carry an optional dormant thread_id, Scenario: EventEnvelope with thread_id round-trips, Scenario: EventEnvelope without thread_id deserializes as None, Scenario: Mutating thread_id invalidates the grant MAC, Scenario: TaskGrant with thread_id round-trips, Scenario: TaskGrant without thread_id deserializes as None (+1 more)

### Community 158 - "D-006 — Identity is not authority"
Cohesion: 0.12
Nodes (26): create_approved_draft(), handle_draft_approval_callback(), Result, Ulid, activate_approved_artifact(), Result, finalize_nomination(), NominationAssertion (+18 more)

### Community 159 - "D-011 — Approval must be digest-bound"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Implement identity store and principal, Summary, What Changes (+1 more)

### Community 160 - "D-012 — Audit stores private payloads by encrypted/hash reference"
Cohesion: 0.20
Nodes (9): 1. Schemas, 2. Identity store, 3. IdentityResolver seam, 4. Composition cutover + bootstrap, 5. Owner-asserted binding path, 6. Tests, 7. Decision log + claims + docs, 8. Validation (+1 more)

### Community 161 - "D-013 — Dynamic behavior easy; dynamic authority hard"
Cohesion: 0.20
Nodes (10): Agent-OS change sequence (2026-07-07, AD canon), Event substrate, implement-counterparty-key-model, implement-durable-workflow-replay, implement-event-bus-subscriptions, implement-overlay-export-restore, implement-overlay-model, implement-task-board (+2 more)

### Community 162 - "D-014 — Bootstrap/setup secrets bypass shell/model context"
Cohesion: 0.22
Nodes (8): 1. Create schema location, 2. Define event schemas, 3. Define identity and route schemas, 4. Define authority schemas, 5. Define action/approval/model/audit schemas, 6. Verification, 7. Review, Tasks: Define core runtime schemas

### Community 163 - "D-015 — Phase 1 should avoid final email send"
Cohesion: 0.22
Nodes (8): 1. Add development-process spec, 2. Add development-process design, 3. Add proposal, 4. Strengthen OpenSpec config, 5. Review consistency with existing PRD and decision log, 6. Prepare future changes, 7. Archive readiness, Tasks: Define OpenSpine development process

### Community 164 - "D-016 — Capability packs are candidate profiles, not live authority"
Cohesion: 0.22
Nodes (8): 1. Immutable draft artifact, 2. Approval record, 3. Gate integration, 4. Gmail draft action, 5. Audit, 6. Tests, 7. Validation, Tasks: Implement digest-bound draft approval

### Community 165 - "D-017 — Personas grant no authority"
Cohesion: 0.22
Nodes (8): 1. Telegram connector, 2. Event normalization, 3. Routing and authority, 4. Actions, 5. Tests, 6. Documentation, 7. Validation, Tasks: Implement Telegram owner control slice

### Community 166 - "D-018 — Routes are declarative artifacts, not kernel code"
Cohesion: 0.22
Nodes (8): 1. ActionCatalog (schemas) + fail-fast wiring (authority, gate), 2. ActionHandlerRegistry (kernel/api), 3. ConnectorRegistry (kernel), 4. Artifact-kind table (kernel), Alternatives considered, Approach, Design: Refactor kernel registries, Key decisions

### Community 167 - "D-020 — Railway/Docker/VPS are deployment targets, not core architecture"
Cohesion: 0.22
Nodes (8): 1. Typed stage sequence — consumed, not decorative, 2. Lanes as data — with a hard hook boundary, 3. Cutover — code moves, it does not accrete, 4. What does NOT move, Alternatives considered, Approach, Design: Refactor pipeline driver, Key decisions (D-054)

### Community 168 - "D-025 — Rust/Tokio substrate: storage, audit chain, and secrets handling"
Cohesion: 0.22
Nodes (8): ADDED Requirements, MODIFIED Requirements, Requirement: Audit schemas MUST reference private payloads by encrypted or hash refs, Requirement: Event bus subscription types MUST be explicit schemas, Scenario: Audit event carries aggregate stream coordinates, Scenario: Filter and checkpoint types exist, Scenario: Model request includes private email content, Spec: Core runtime schemas (event bus extensions)

### Community 169 - "D-027 — Multi-provider model gateway with per-provider auth mode"
Cohesion: 0.42
Nodes (8): derived_lineage_round_trips_on_artifact_row(), inconsistent_generation_zero_with_parents_is_rejected(), inconsistent_positive_generation_without_parents_is_rejected(), lineage_is_distinct_from_version(), root_lineage_round_trips_on_artifact_row(), row_with(), stored_inconsistent_lineage_fails_closed_on_load(), unknown_lineage_is_none_not_root()

### Community 170 - "D-028 — Canonical artifact format is YAML+serde; digests are `sha256:` over canonical JSON"
Cohesion: 0.46
Nodes (7): approve_callback(), learned_route(), nomination_owner_tap_persists_nominated_status_and_audit(), nomination_requires_explicit_depersonalized_assertion(), MockServer, Ulid, telegram_ok()

### Community 171 - "D-029 — Gmail OAuth scopes: `readonly` + `compose`, send hard-denied at the gate"
Cohesion: 0.15
Nodes (12): Consequences, Consequences, D-038 — `resolve_owner_identity`'s `channel_trust` is caller-supplied, not hardcoded, D-049 — Capability specs are backfilled for subsystems implemented inside earlier slices, Decision, Decision, Decision Index, Lyra PRD Companion — Decisions Log (+4 more)

### Community 172 - "openspine-decision-log.md"
Cohesion: 0.25
Nodes (7): Built, Canon deviations / narrowings, Exact local gate results, Implementation Notes: implement-overlay-model, Ratified decisions, Rebase hotspots, Review remediation state

### Community 173 - "D-031 — Docker Compose is the first reference deployment target"
Cohesion: 0.25
Nodes (7): Context, Design: Authority composition, Inputs, Merge rule, Output, Precedence, Tests

### Community 174 - "D-032 — Kernel↔shell transport is HTTP/JSON with a per-task bearer token"
Cohesion: 0.25
Nodes (7): 1. New capability specs, 2. Restore dropped dev-process requirements, 3. Close the loophole going forward, 4. Docs, 5. Decision log, 6. Validation, Tasks: Backfill implemented capability specs

### Community 175 - "D-033 — Action identifiers are exact-match dotted strings; unverified senders are audited and ignored"
Cohesion: 0.25
Nodes (7): ADDED Requirements, Requirement: Approval MUST bind only what the owner was shown, Requirement: Approvals MUST expire, Scenario: Approval has expired, Scenario: Preview fits without truncation, Scenario: Preview must be truncated, Spec: Digest-bound draft approval

### Community 176 - "D-034 — `email.create_draft` is the one canonical action id; the qualified PRD §10.2 spelling is dropped"
Cohesion: 0.25
Nodes (7): ADDED Requirements, Requirement: Grant limits MUST be enforced at runtime, Requirement: Kernel-originated owner notifications are a trusted, audited path, Scenario: Kernel sends a courtesy notice, Scenario: Model call beyond the budget, Scenario: Shell-initiated artifact creation beyond the budget, Spec: Gate action API

### Community 177 - "D-035 — Kernel advertises a separate `advertise_endpoint` from its `bind_addr`; no Unix-domain-socket transport for `ProcessDriver`"
Cohesion: 0.25
Nodes (7): 1. ActionCatalog + fail-fast, 2. ConnectorRegistry, 3. ActionHandlerRegistry, 4. Artifact-kind table, 5. Decision log + docs, 6. Validation, Tasks: Refactor kernel registries

### Community 178 - "D-010 — Model calls with private context go through model gateway"
Cohesion: 0.25
Nodes (7): 1. Typed stage sequence, 2. LaneSpec + lane constructors, 3. Driver + cutover, 4. Tests, 5. Decision log + docs, 6. Validation, Tasks: Refactor pipeline driver

### Community 179 - "D-019 — Implement minimal slice first, not full agent OS"
Cohesion: 0.25
Nodes (7): Alternatives considered, Authority posture, Design: define-lineage-and-eval-store, Eval-verdict store, Lineage model, Migration strategy, Risks

### Community 180 - "why-openspine.md"
Cohesion: 0.25
Nodes (7): Acceptance Criteria, Dependencies, Out of Scope, Problem/Context, Proposal: Implement escalation and refusal, Proposed Solution, Summary

### Community 181 - "Q: Trace the full runtime call path for a single incoming Telegram message from an authorized owner, all the way through to a TaskGrant being persisted in the kernel's SQLite store. List the modules/files involved, in call order, and identify the 2-3 modules that are most structurally central to this flow."
Cohesion: 0.25
Nodes (7): Acceptance Criteria, Context, Dependencies, Out of Scope, Problem, Proposal: Model swap ceremony, Proposed Solution

### Community 182 - "Blindspot pass"
Cohesion: 0.25
Nodes (7): MODIFIED Requirements, Requirement: ActionCatalog MUST enumerate every trusted-path carve-out around gate(), Scenario: Each enumerated entry has a dedicated test, Scenario: Model activation is classified, Scenario: Model golden-set execution is classified, Scenario: The carve-out set is finite and enumerated, Spec: Gate action API

### Community 183 - "Brainstorm and prototypes"
Cohesion: 0.17
Nodes (11): Purpose, Requirement: Configurable caps, Requirement: Global daily spend ledger, Requirement: Lane-aware breach boundary, Requirements, Scenario: Concurrent reservation cannot overspend, Scenario: Configured caps are enforced, Scenario: Counters persist by UTC day (+3 more)

### Community 185 - "Implementation notes"
Cohesion: 0.25
Nodes (7): Acceptance Criteria, Context, Dependencies, Out of Scope, Problem, Proposal: Overlay evaluation gate, Proposed Solution

### Community 186 - "Implementation plan"
Cohesion: 0.25
Nodes (7): Acceptance Criteria, Dependencies, Impact, Out of Scope, Problem/Context, Proposal: Plan digest-bound approval, Proposed Solution

### Community 187 - "Interview me"
Cohesion: 0.25
Nodes (7): Acceptance Criteria, Dependencies, Out of Scope, Problem, Proposal: Implement durable workflow replay, Proposed Solution, Ratified decisions

### Community 188 - "Pitch packager"
Cohesion: 0.25
Nodes (7): 1. OpenSpec and authority, 2. Encrypted vault, 3. Gate-mediated intake mode, 4. Connector wiring, 5. Acceptance tests, 6. Verification, Tasks: Implement secret intake

### Community 189 - "Reference hunt"
Cohesion: 0.25
Nodes (8): Authority-sensitive changes, Change structure, Naming, OpenSpec boundary, OpenSpine conventions, Purpose, Requirement language, Verification

### Community 190 - "template"
Cohesion: 0.25
Nodes (8): Docs, Every claim has a test, License, OpenSpine, Status, Try it in 5 minutes, What is this?, Why it's different

### Community 191 - "architecture.md"
Cohesion: 0.48
Nodes (5): grant(), legacy_missing_chain_defaults_but_fails_closed(), legacy_without_thread_id_defaults_to_none(), round_trip_and_mac(), thread_id_round_trips_when_populated()

### Community 192 - "decisions.md"
Cohesion: 0.29
Nodes (6): ArtifactNominatePayload, dispatch_artifact_nominate(), Option, Result, String, Value

### Community 193 - "quickstart.md"
Cohesion: 0.52
Nodes (6): accepted_dependency_fingerprint_migration_is_idempotent(), accepted_dependency_fingerprint_round_trips(), base_namespace_cannot_be_recorded_as_learned(), learned_artifact_round_trip_preserves_typed_provenance(), owner_accepted_is_durable_across_round_trip(), sample()

### Community 194 - "roadmap.md"
Cohesion: 0.29
Nodes (6): Acceptance Criteria, Dependencies, Out of Scope, Problem/Context, Proposal: Implement overlay artifact compatibility, Proposed Solution

### Community 195 - "threat-model.md"
Cohesion: 0.51
Nodes (10): email_reply_drafter_grant(), email_reply_drafter_template_wraps_untrusted_context_on_the_wire(), grant_with_limits(), max_artifacts_of_one_denies_the_second_call_with_a_single_provider_hit(), max_model_calls_of_one_denies_the_second_call_with_a_single_provider_hit(), post_model_generate(), start_server(), state_with_mock_provider() (+2 more)

### Community 196 - "tsconfig.json"
Cohesion: 0.29
Nodes (6): 1. Create the Telegram bot, 2. Find your Telegram user ID — owner identity, verified structurally, 3. Generate the artifact encryption key, 4. Minimal `openspine.yaml`, 5. Unsafe dev shortcuts (do not carry into production), Telegram owner-control setup (Phase 1)

### Community 197 - "editUrl"
Cohesion: 0.29
Nodes (6): 1. Composer interface, 2. Merge logic, 3. Tests, 4. Documentation, 5. Validation, Tasks: Implement authority composition

### Community 198 - "head"
Cohesion: 0.29
Nodes (6): Approval record, Design: Digest-bound draft approval, Final send, Flow, Gate behavior, Immutable artifact

### Community 199 - "pagefind"
Cohesion: 0.29
Nodes (6): Audit, Behavior, Connector execution, Design: Gate action API, Gate responsibility, Interface

### Community 200 - "AGENTS.md"
Cohesion: 0.29
Nodes (6): 1. Types, 2. Gate implementation, 3. Audit, 4. Tests, 5. Validation, Tasks: Implement gate action API

### Community 201 - "AGENTS.md"
Cohesion: 0.29
Nodes (6): Allowed actions, Design: Selected-thread email preview slice, Email content trust, Flow, Output, Selection token

### Community 202 - "proposal.rs"
Cohesion: 0.29
Nodes (6): Design: Telegram owner control slice, Flow, Main assistant authority, Polling vs webhook, Secret intake, Verification

### Community 203 - "autoresearch.sh"
Cohesion: 0.29
Nodes (6): ADDED Requirements, Requirement: Selection tokens MUST be single-use, expiring, and scope-fixed, Scenario: Token reused after consumption, Scenario: Token used after expiry, Scenario: Token used by a foreign grant, Spec: Selected-thread email preview slice

### Community 204 - "CLAUDE.md"
Cohesion: 0.29
Nodes (6): Chained HMAC construction, Design: Define grant chain and modes, Gate decisions and shadow, Immutable root authority + append-only caveats, Macaroons-simple chain (AD-101), Out of scope

### Community 205 - "README.md"
Cohesion: 0.29
Nodes (6): MODIFIED Requirements, Requirement: Task grants MUST be explicit live authority objects, Scenario: Bound parameters are caveats, Scenario: Root grant defaults, Scenario: Sub-grant is the sole presented authority, Spec: Core runtime schemas

### Community 207 - "check-claims.sh"
Cohesion: 0.29
Nodes (6): Acceptance Criteria, Change: define-lineage-and-eval-store, Dependencies, Out of Scope, Problem/Context, Proposed Solution

### Community 208 - "check-file-sizes.sh"
Cohesion: 0.29
Nodes (6): Alternatives rejected, Authority boundary, Compatibility, Design: Egress classes, Grant and MAC model, Registry ratings

### Community 209 - "README.md"
Cohesion: 0.29
Nodes (6): Acceptance Criteria, Dependencies, Out of Scope, Problem/Context, Proposal: Implement egress classes, Proposed Solution

### Community 211 - "index.mdx"
Cohesion: 0.29
Nodes (6): 1. OpenSpec artifacts, 2. Schema types (openspine-schemas), 3. Construction site updates, 4. Kernel integration (openspine-kernel), 5. Local gate, Tasks: implement-escalation-and-refusal

### Community 213 - "apply.md"
Cohesion: 0.29
Nodes (6): Acceptance Criteria, Dependencies, Out of Scope, Problem/Context, Proposal: Implement event bus subscriptions, Proposed Solution

### Community 214 - "archive.md"
Cohesion: 0.29
Nodes (6): ADDED Requirements, Requirement: Audit append MUST assign per-aggregate sequence under the store lock, Requirement: The store MUST support filtered ordered replay of the audit ledger, Scenario: Replay after watermark skips earlier rows, Scenario: Sequential appends on one aggregate, Spec: Audit artifact store (event bus ledger extensions)

### Community 215 - "propose.md"
Cohesion: 0.29
Nodes (6): 1. Schemas, 2. Ledger write path, 3. Replay + idempotent consumer, 4. Tests, 5. Validation, Tasks: Implement event bus subscriptions

### Community 217 - "SKILL.md"
Cohesion: 0.29
Nodes (6): MODIFIED Requirements, Requirement: Identity schemas MUST NOT grant runtime authority, Requirement: OpenSpine core runtime objects MUST have explicit schemas, Scenario: Known owner identity exists, Scenario: Runtime object is added, Spec: Core runtime schemas

### Community 218 - "SKILL.md"
Cohesion: 0.29
Nodes (6): Authority boundary, Canonical plan payload, Design: Plan digest-bound approval, Mutation refusal, One-loop question and kernel response, Verification strategy

### Community 219 - "SKILL.md"
Cohesion: 0.29
Nodes (6): Design: Durable workflow replay, Exact step identity, Gated, approval, and private boundaries, Ledger aggregate and verified replay, Ratified decisions, Timer substrate

### Community 220 - "SKILL.md"
Cohesion: 0.29
Nodes (6): Acceptance Criteria, Dependencies, Failure surfacing contract, Out of Scope, Problem/Context, Proposed Solution

### Community 221 - "SKILL.md"
Cohesion: 0.20
Nodes (9): 1. Versioned Schema Migrations (`PRAGMA user_version`), 2. Boot Clock-Regression Detection, 3. Same-Conversation Serialization Guard, 4. Audit I/O Verification (Read-Only DB), 5. Backup & Restore Scope, Centralized Initialization & Safety Check, Design: Day-2 Operations Contract, Transactional Atomicity (+1 more)

### Community 222 - "SKILL.md"
Cohesion: 0.20
Nodes (9): ADDED Requirements, Requirement: Configurable caps, Requirement: Global daily spend ledger, Requirement: Lane-aware breach boundary, Scenario: Concurrent reservation cannot overspend, Scenario: Configured caps are enforced, Scenario: Counters persist by UTC day, Scenario: Immediate owner notification (+1 more)

### Community 224 - "lib.rs"
Cohesion: 0.22
Nodes (8): Acceptance Criteria, Dependencies, Non-Goals, Problem, Proposal: Implement Day-2 Operations Contract, Proposed Solution, Ratified decisions, Scope

### Community 242 - "lib.rs"
Cohesion: 0.25
Nodes (8): Change Log, Consequences, D-081 — Upstream nomination is explicit depersonalized opt-in, Decision, Open Decision Questions — CLOSED (see linked decisions), Rationale, Research / Reference Backlog, Would change if

### Community 250 - "opsx-apply.md"
Cohesion: 0.25
Nodes (7): Audit I/O failure handling, Consistent backup and restore, Day-2 Operations Contract, Failure messages and boundaries, First-run and restart sequence, Schema migrations (`PRAGMA user_version`), Telegram-first credential and connector sequence (AD-144)

### Community 251 - "opsx-archive.md"
Cohesion: 0.29
Nodes (5): Receiver, Request, Respond, Sender, FirstCallbackGate

### Community 252 - "opsx-propose.md"
Cohesion: 0.29
Nodes (6): 1. Create a Google Cloud OAuth client, 2. Obtain a refresh token, 3. `openspine.yaml`'s `gmail` block, 4. Selecting a thread — the `/draft <thread_id>` command, 5. Unsafe dev shortcuts (do not carry into production), Gmail selected-thread email preview setup (Phase 2)

### Community 253 - "SKILL.md"
Cohesion: 0.43
Nodes (6): dispatch_polled_updates_for_test(), initialize_telegram_bot_id(), initialize_telegram_bot_id_until_ready(), is_already_processed(), resolve_telegram_offset(), resolve_telegram_offset_for_test()

### Community 254 - "SKILL.md"
Cohesion: 0.33
Nodes (6): Agentic decisions, D-008 — Deterministic routing decides authority; agentic routing decides strategy, Decision, Deterministic decisions, Rationale, Would change if

### Community 268 - "content-assets.mjs"
Cohesion: 0.33
Nodes (6): Consequences, D-001 — Lyra is a runtime/substrate, not a single agent, Decision, Rationale, Trade-offs, Would change if

### Community 269 - "content-modules.mjs"
Cohesion: 0.33
Nodes (6): Consequences, D-039 — Draft-approval channel is a Telegram inline button (`callback_query`), not a text command, Decision, Rationale, Trade-offs, Would change if

### Community 270 - "types.d.ts"
Cohesion: 0.33
Nodes (6): Consequences, D-042 — Reply recipient is kernel-derived, never shell-supplied: newest non-owner sender, matched against a configured mailbox address, Decision, Rationale, Trade-offs, Would change if

### Community 274 - "SKILL.md"
Cohesion: 0.33
Nodes (6): Consequences, D-002 — First usable UX should include an owner control channel, Decision, Rationale, Trade-offs, Would change if

### Community 275 - "opsx-explore.md"
Cohesion: 0.29
Nodes (7): Authority growth, implement-disclosure-policy, implement-egress-classes, implement-model-swap-ceremony, implement-overlay-eval-gate, implement-plan-digest-approval, implement-standing-rules

### Community 276 - "opsx-apply.md"
Cohesion: 0.29
Nodes (7): define-grant-chain-and-modes, define-lineage-and-eval-store, harden-gate-trusted-paths, implement-identity-store-and-principal, Kernel foundation, refactor-kernel-registries, refactor-pipeline-driver

### Community 277 - "opsx-archive.md"
Cohesion: 0.33
Nodes (5): Budget enforcement placement (D-046), Design: Harden approval and budgets, Task-token hashing and sweep (D-047), Trusted notification audit (D-046 continued), WYSIWYS (D-045)

### Community 278 - "opsx-propose.md"
Cohesion: 0.33
Nodes (5): Acceptance Criteria, Non-goals, Proposal: Define grant chain and modes, Summary, What Changes

### Community 279 - "SKILL.md"
Cohesion: 0.33
Nodes (5): ADDED Requirements, Requirement: Authority-bearing proposals require overlay evaluation before approval, Scenario: Generic lifecycle bypass is rejected, Scenario: Proposal with two passing evaluations reaches approval, Scenario: Proposal without captured owner history is denied

### Community 280 - "SKILL.md"
Cohesion: 0.33
Nodes (6): Failure surfacing & operations, implement-connector-reality, implement-day2-operations, implement-failure-surfacing-contract, implement-secret-intake, implement-spend-kill-switch

### Community 281 - "SKILL.md"
Cohesion: 0.45
Nodes (12): add_column_if_missing(), apply_ad_hoc_migrations(), apply_single_migration_for_test(), apply_single_migration_inner(), apply_versioned_migrations(), latest_user_version(), read_user_version(), revert_versioned_migrations_for_test() (+4 more)

### Community 283 - "ADDED Requirements"
Cohesion: 0.40
Nodes (4): Authority-sensitive decisions, Design: Base/overlay compatibility, Failure behavior, Lifecycle state machine

### Community 284 - "Proposal: Refactor kernel registries"
Cohesion: 0.40
Nodes (4): Approach, Design: Backfill implemented capability specs, Dev-process restoration, Forward-looking requirement

### Community 285 - "Approach"
Cohesion: 0.33
Nodes (6): Consequences, D-003 — Gmail is a guarded workflow, not the whole product, Decision, Rationale, Trade-offs, Would change if

### Community 286 - "Tasks: Refactor kernel registries"
Cohesion: 0.33
Nodes (6): Consequences, D-004 — Every effectful action goes through `gate()`, Decision, Effectful actions include, Rationale, Would change if

### Community 287 - "ApprovalRecord"
Cohesion: 0.33
Nodes (6): Consequences, D-005 — Private-data shell must be contained, Decision, Rationale, Required containment, Would change if

### Community 288 - "fixtures.rs"
Cohesion: 0.33
Nodes (6): Consequences, D-009 — External content is data, not instruction, Decision, Examples, Rationale, Would change if

### Community 289 - "D-052 — Archive applies deltas mechanically via `openspec archive --yes`; pre-seeded requirements are carried as MODIFIED; the `--skip-specs` hand-apply ceremony is retired"
Cohesion: 0.33
Nodes (6): Consequences, D-021 — Email domain is broader than Gmail, Decision, Rationale, Trade-offs, Would change if

### Community 290 - "D-030 — Telegram carries the entire owner-control UX for phases 1–3"
Cohesion: 0.33
Nodes (6): Consequences, D-022 — Agent-owned inbox is distinct from owner mailbox access, Decision, Distinction, Rationale, Would change if

### Community 291 - "artifact_propose.rs"
Cohesion: 0.33
Nodes (6): Consequences, D-023 — OpenSpine is the substrate; Lyra is a product built on it, Decision, Positioning, Rationale, Would change if

### Community 292 - "Overlay & key model"
Cohesion: 0.33
Nodes (6): Consequences, D-024 — OpenSpec is the development/change-management layer, not the runtime, Decision, Mapping, Rationale, Would change if

### Community 293 - "Proposal: Define grant chain and modes"
Cohesion: 0.33
Nodes (6): Consequences, D-026 — Shell containment via a `SandboxDriver` trait (Process dev-only / Docker), Decision, Rationale, Trade-offs, Would change if

### Community 294 - "D-010 — Model calls with private context go through model gateway"
Cohesion: 0.33
Nodes (5): Boundary, Configuration, Design, Ledger, Model and connector accounting

### Community 296 - "Q: Trace the full runtime call path for a single incoming Telegram message from an authorized owner, all the way through to a TaskGrant being persisted in the kernel's SQLite store. List the modules/files involved, in call order, and identify the 2-3 modules that are most structurally central to this flow."
Cohesion: 0.40
Nodes (5): Consequences, D-035 — Kernel advertises a separate `advertise_endpoint` from its `bind_addr`; no Unix-domain-socket transport for `ProcessDriver`, Decision, Rationale, Would change if

### Community 297 - "Blindspot pass"
Cohesion: 0.40
Nodes (5): Consequences, D-036 — Phase-2 thread selection is a kernel-recognized `/draft <thread_id>` command, not free-form NLU or a shell-supplied id, Decision, Rationale, Would change if

### Community 298 - "Delegation & containment"
Cohesion: 0.40
Nodes (5): Consequences, D-037 — Gmail OAuth via a plain refresh-token POST (no `oauth2` crate); `base64` promoted from transitive to direct dependency, Decision, Rationale, Would change if

### Community 299 - "SelectionToken"
Cohesion: 0.40
Nodes (5): Consequences, D-040 — Pending (pre-approval) `ActionRequest`s are persisted in a new `action_requests` table, Decision, Rationale, Would change if

### Community 300 - "tasks.md"
Cohesion: 0.40
Nodes (5): Consequences, D-041 — `email.create_draft`'s digest composition: payload = `{subject, body}`, target = `{thread_id, connector, account_role, recipients}`, Decision, Rationale, Would change if

### Community 301 - "Implementation notes"
Cohesion: 0.40
Nodes (5): Consequences, D-043 — `lyra.ui.preview` is extended (not duplicated) to propose the exact reviewed draft and attach the approval button, Decision, Rationale, Would change if

### Community 302 - "tests.rs"
Cohesion: 0.40
Nodes (5): Consequences, D-044 — Approved draft creation dispatches kernel-side; no new shell spawn, Decision, Rationale, Would change if

### Community 303 - "get_task"
Cohesion: 0.40
Nodes (5): Consequences, D-045 — WYSIWYS: a truncated preview refuses an approval button rather than splitting the message, Decision, Rationale, Would change if

### Community 304 - "Approach"
Cohesion: 0.40
Nodes (5): Consequences, D-046 — Grant budgets are enforced kernel-dispatch-side; the artifact budget counts only shell-initiated puts, Decision, Rationale, Would change if

### Community 305 - "Approach"
Cohesion: 0.40
Nodes (5): Consequences, D-047 — Task tokens are hashed at rest; expired grants are swept, Decision, Rationale, Would change if

### Community 306 - "action_catalog.rs"
Cohesion: 0.40
Nodes (5): Consequences, D-048 — `artifact.activate` is the single canonical activation action id; every runtime proposal requires uniform owner approval; prompt templates are excluded from proposable kinds, Decision, Rationale, Would change if

### Community 307 - ".fmt"
Cohesion: 0.40
Nodes (5): Consequences, D-050 — `max_model_calls` is enforced with an atomic upsert, not a count-then-compare, Decision, Rationale, Would change if

### Community 308 - "artifact_propose.rs"
Cohesion: 0.40
Nodes (5): Consequences, D-051 — The agent-OS canon (AD-001..153) is decomposed into a dependency-edged change sequence; the stale later-changes placeholders are superseded or subsumed, Decision, Rationale, Would change if

### Community 309 - "effect_paths.rs"
Cohesion: 0.40
Nodes (5): Consequences, D-052 — Archive applies deltas mechanically via `openspec archive --yes`; pre-seeded requirements are carried as MODIFIED; the `--skip-specs` hand-apply ceremony is retired, Decision, Rationale, Would change if

### Community 310 - "Overlay & key model"
Cohesion: 0.40
Nodes (5): Consequences, D-053 — Kernel extension points are compiled-in registries; a curated canonical `ActionCatalog` makes unknown action ids fail fast at composition and gate, Decision, Rationale, Would change if

### Community 311 - "retry_worker.rs"
Cohesion: 0.40
Nodes (5): Consequences, D-054 — Pipeline stages are a typed compiled-in sequence the driver executes; lanes are compiled-in data records, Decision, Rationale, Would change if

### Community 312 - "D-014 — Bootstrap/setup secrets bypass shell/model context"
Cohesion: 0.40
Nodes (5): Consequences, D-055 — Gate trusted paths are hardened: carve-outs are enumerated catalog data; KernelOrigin is approval-exempt, audit-never-exempt; selection-token validation lives in pure gate() with dispatch-side consumption; digests are kernel-re-derived at approval-effect time, Decision, Rationale, Would change if

### Community 313 - "editUrl"
Cohesion: 0.40
Nodes (5): Consequences, D-056 — Eval-store groundwork defers AD-111 evaluator policy: only the verdict-landing surface is settled, Decision, Rationale, Would change if

### Community 314 - "Proposal: Implement identity store and principal"
Cohesion: 0.40
Nodes (5): Consequences, D-057 — Counterparty-facing actions are an explicit kernel catalog set, Decision, Rationale, Would change if

### Community 315 - "Tasks: Implement identity store and principal"
Cohesion: 0.40
Nodes (5): Consequences, D-058 — Security escalations require result-returning owner delivery, Decision, Rationale, Would change if

### Community 316 - "Option"
Cohesion: 0.40
Nodes (5): Consequences, D-059 — Dormant thread bindings are MAC-authenticated before activation, Decision, Rationale, Would change if

### Community 317 - "Ulid"
Cohesion: 0.40
Nodes (5): Consequences, D-060 — The overlay eval gate's first-cut evaluator is deterministic; the full replay/judge protocol is owner-reserved, Decision, Rationale, Would change if

### Community 318 - "Vec"
Cohesion: 0.40
Nodes (5): Consequences, D-061 — Model-swap golden sets use a bounded deterministic first cut, Decision, Rationale, Would change if

### Community 319 - "HashMap"
Cohesion: 0.40
Nodes (5): Consequences, D-062 — Active model swaps require symmetric DB and overlay provenance, Decision, Rationale, Would change if

### Community 320 - "benchmark.rs"
Cohesion: 0.40
Nodes (5): Consequences, D-063 — Model-swap activation uses a serialized staged recovery protocol, Decision, Rationale, Would change if

### Community 321 - "Timestamp"
Cohesion: 0.40
Nodes (5): Consequences, D-064 — Connector secrets migrate once into the kernel vault, Decision, Rationale, Would change if

### Community 322 - "head"
Cohesion: 0.40
Nodes (5): Consequences, D-065 — Provider API-key vault migration belongs to foundation amendment, Decision, Rationale, Would change if

### Community 323 - "Vec"
Cohesion: 0.40
Nodes (5): Consequences, D-066 — Paired Gmail credentials stage until atomic validated promotion, Decision, Rationale, Would change if

### Community 324 - "lib.rs"
Cohesion: 0.40
Nodes (5): Consequences, D-067 — Telegram poll offsets are namespaced by bot identity, Decision, Rationale, Would change if

### Community 325 - "Arc"
Cohesion: 0.40
Nodes (5): Consequences, D-068 — Authenticated API bad requests are not duplicated through owner notification, Decision, Rationale, Would change if

### Community 326 - "Error"
Cohesion: 0.40
Nodes (5): Consequences, D-069 — Kernel connector counters are the minimal observability surface, Decision, Rationale, Would change if

### Community 327 - "D-015 — Phase 1 should avoid final email send"
Cohesion: 0.40
Nodes (5): Consequences, D-070 — Retryable owner notifications use encrypted artifact references, Decision, Rationale, Would change if

### Community 328 - "D-016 — Capability packs are candidate profiles, not live authority"
Cohesion: 0.40
Nodes (5): Consequences, D-071 — External owner delivery may be delivery-unknown after a crash, Decision, Rationale, Would change if

### Community 329 - "Option"
Cohesion: 0.40
Nodes (5): Consequences, D-072 — Digest detail retrieval is a secure lossless pagination substrate, Decision, Rationale, Would change if

### Community 330 - "Result"
Cohesion: 0.40
Nodes (5): Consequences, D-073 — Durable workflow steps persist intent before effect and replay recorded outcomes, Decision, Rationale, Would change if

### Community 331 - "State"
Cohesion: 0.40
Nodes (5): Consequences, D-074 — Workflow timers fire at most once via trusted-clock atomic claims, Decision, Rationale, Would change if

### Community 332 - "StatusCode"
Cohesion: 0.40
Nodes (5): Consequences, D-075 — The spend kill switch accounts globally but pauses only non-immediate lanes, Decision, Rationale, Would change if

### Community 333 - "String"
Cohesion: 0.40
Nodes (5): Consequences, D-076 — Spend caps are required finite configuration, Decision, Rationale, Would change if

### Community 334 - "Ulid"
Cohesion: 0.40
Nodes (5): Consequences, D-077 — Learned artifacts carry exchange provenance and reconfirmations record a durable anchor, Decision, Rationale, Would change if

### Community 335 - "Value"
Cohesion: 0.40
Nodes (5): Consequences, D-078 — Owner reconfirmation is digest-bound with a durable owner-accepted disposition, Decision, Rationale, Would change if

### Community 336 - "MockServer"
Cohesion: 0.40
Nodes (5): Consequences, D-079 — Overlay compatibility converges to a fixed point and base wins identity collisions, Decision, Rationale, Would change if

### Community 337 - "TelegramConnector"
Cohesion: 0.40
Nodes (5): Consequences, D-080 — Legacy migration is discovery and quarantine only, Decision, Rationale, Would change if

### Community 338 - "D-032 — Kernel↔shell transport is HTTP/JSON with a per-task bearer token"
Cohesion: 0.40
Nodes (5): Consequences, D-006 — Identity is not authority, Decision, Rationale, Would change if

### Community 340 - "D-033 — Action identifiers are exact-match dotted strings; unverified senders are audited and ignored"
Cohesion: 0.40
Nodes (5): Consequences, D-007 — Task grant is the final runtime authority, Decision, Rationale, Would change if

### Community 341 - "MockServer"
Cohesion: 0.40
Nodes (5): Consequences, D-011 — Approval must be digest-bound, Decision, Rationale, Would change if

### Community 342 - "Timestamp"
Cohesion: 0.40
Nodes (5): Consequences, D-012 — Audit stores private payloads by encrypted/hash reference, Decision, Rationale, Would change if

### Community 343 - "Value"
Cohesion: 0.40
Nodes (5): Consequences, D-013 — Dynamic behavior easy; dynamic authority hard, Decision, Rationale, Would change if

### Community 344 - "Arc"
Cohesion: 0.40
Nodes (4): Alternatives considered, Approach, Design: Artifact lifecycle slice, Key decisions

### Community 345 - "HeaderMap"
Cohesion: 0.40
Nodes (4): Canon and audited sites, Design, Routing, Storage

### Community 346 - "D-030 — Telegram carries the entire owner-control UX for phases 1–3"
Cohesion: 0.40
Nodes (5): Delegation & containment, implement-briefcase-packing, implement-escalation-and-refusal, implement-worker-runtime, implement-worker-supervision

### Community 347 - "Option"
Cohesion: 0.40
Nodes (5): implement-authority-equivalence-matcher, implement-seed-workflows, implement-skill-artifact-class, implement-workflow-state-machines, Skills & workflows

### Community 348 - "Result"
Cohesion: 0.40
Nodes (5): implement-nerve-subscribers, implement-persona-binding-and-headless-lanes, implement-personality-seed, implement-reflection-miner, Reflection & product surface

### Community 351 - "String"
Cohesion: 0.40
Nodes (4): devDependencies, @fission-ai/openspec, name, private

### Community 354 - "JoinHandle"
Cohesion: 0.40
Nodes (4): Safe rules for changing behavior, The bet, The problem, What OpenSpine does not do on purpose

### Community 356 - "Response"
Cohesion: 0.83
Nodes (3): Cli, main(), run()

### Community 357 - "SocketAddr"
Cohesion: 0.50
Nodes (3): Answer, Q: Trace the full runtime call path for a single incoming Telegram message from an authorized owner, all the way through to a TaskGrant being persisted in the kernel's SQLite store. List the modules/files involved, in call order, and identify the 2-3 modules that are most structurally central to this flow., Source Nodes

### Community 358 - "HashMap"
Cohesion: 0.50
Nodes (3): Blindspot pass, Guardrails, Steps

### Community 359 - "Option"
Cohesion: 0.50
Nodes (3): Brainstorm and prototypes, Guardrails, Steps

### Community 360 - "Self"
Cohesion: 0.50
Nodes (3): Change quiz, Guardrails, Steps

### Community 361 - "Value"
Cohesion: 0.50
Nodes (3): Guardrails, Implementation notes, Steps

### Community 362 - "Arc"
Cohesion: 0.50
Nodes (3): Guardrails, Implementation plan, Steps

### Community 363 - "Display"
Cohesion: 0.50
Nodes (3): Guardrails, Interview me, Steps

### Community 364 - "HeaderMap"
Cohesion: 0.50
Nodes (3): Guardrails, Pitch packager, Steps

### Community 365 - "Json"
Cohesion: 0.22
Nodes (3): ArtifactNamespace, ArtifactRef, can_transition()

### Community 366 - "Option"
Cohesion: 0.50
Nodes (3): Guardrails, Reference hunt, Steps

### Community 367 - "Result"
Cohesion: 0.83
Nodes (3): forbid(), require(), check-omp-ceremony.sh script

### Community 368 - "State"
Cohesion: 0.50
Nodes (3): Crate map, The kernel/shell trust boundary, The pipeline

### Community 369 - "StatusCode"
Cohesion: 0.50
Nodes (3): A worked example: catching a real approval bypass, Reading the log, Why a decision log

### Community 370 - "Ulid"
Cohesion: 0.50
Nodes (3): Build and run the check script, Configure a real server, Try it

### Community 371 - "Value"
Cohesion: 0.50
Nodes (3): Backfilled, Deferred, on purpose, Shipped

### Community 372 - "Vec"
Cohesion: 0.50
Nodes (3): Claims, Claims vs exclusions, honestly, What the current phases do not claim to defend against

### Community 373 - "Arc"
Cohesion: 0.50
Nodes (3): exclude, extends, include

### Community 394 - "Value"
Cohesion: 0.40
Nodes (5): Consequences, D-014 — Bootstrap/setup secrets bypass shell/model context, Decision, Rationale, Would change if

### Community 425 - "HashMap"
Cohesion: 0.40
Nodes (5): Consequences, D-015 — Phase 1 should avoid final email send, Decision, Rationale, Would change if

### Community 426 - "Option"
Cohesion: 0.40
Nodes (5): Consequences, D-016 — Capability packs are candidate profiles, not live authority, Decision, Rationale, Would change if

### Community 427 - "Path"
Cohesion: 0.40
Nodes (5): Consequences, D-017 — Personas grant no authority, Decision, Rationale, Would change if

### Community 459 - "Vec"
Cohesion: 0.40
Nodes (5): Consequences, D-018 — Routes are declarative artifacts, not kernel code, Decision, Rationale, Would change if

### Community 460 - "GmailConnector"
Cohesion: 0.40
Nodes (5): Consequences, D-020 — Railway/Docker/VPS are deployment targets, not core architecture, Decision, Rationale, Would change if

### Community 461 - "MockServer"
Cohesion: 0.40
Nodes (5): Consequences, D-025 — Rust/Tokio substrate: storage, audit chain, and secrets handling, Decision, Rationale, Would change if

### Community 462 - "Value"
Cohesion: 0.40
Nodes (5): Consequences, D-027 — Multi-provider model gateway with per-provider auth mode, Decision, Rationale, Would change if

### Community 463 - "Option"
Cohesion: 0.40
Nodes (5): Consequences, D-028 — Canonical artifact format is YAML+serde; digests are `sha256:` over canonical JSON, Decision, Rationale, Would change if

### Community 464 - "Result"
Cohesion: 0.40
Nodes (5): Consequences, D-029 — Gmail OAuth scopes: `readonly` + `compose`, send hard-denied at the gate, Decision, Rationale, Would change if

### Community 465 - "Self"
Cohesion: 0.40
Nodes (5): Consequences, D-030 — Telegram carries the entire owner-control UX for phases 1–3, Decision, Rationale, Would change if

### Community 472 - "Client"
Cohesion: 0.40
Nodes (5): Consequences, D-031 — Docker Compose is the first reference deployment target, Decision, Rationale, Would change if

### Community 473 - "Error"
Cohesion: 0.40
Nodes (5): Consequences, D-032 — Kernel↔shell transport is HTTP/JSON with a per-task bearer token, Decision, Rationale, Would change if

### Community 476 - "String"
Cohesion: 0.40
Nodes (5): Consequences, D-033 — Action identifiers are exact-match dotted strings; unverified senders are audited and ignored, Decision, Rationale, Would change if

### Community 479 - "Result"
Cohesion: 0.40
Nodes (5): Consequences, D-034 — `email.create_draft` is the one canonical action id; the qualified PRD §10.2 spelling is dropped, Decision, Rationale, Would change if

### Community 480 - "Option"
Cohesion: 0.40
Nodes (5): D-010 — Model calls with private context go through model gateway, Decision, Gateway responsibilities, Rationale, Would change if

### Community 481 - "Result"
Cohesion: 0.40
Nodes (5): D-019 — Implement minimal slice first, not full agent OS, Decision, Minimal slice, Rationale, Would change if

### Community 482 - "String"
Cohesion: 0.40
Nodes (4): Acceptance Criteria, Global daily spend kill switch, What Changes, Why

### Community 483 - "Timestamp"
Cohesion: 0.50
Nodes (3): Implementation, Tasks: Implement Day-2 Operations Contract, Verification

## Knowledge Gaps
- **1693 isolated node(s):** `autoresearch.sh script`, `TelegramReplyPayload`, `ReadThreadPayload`, `TokenResponse`, `GmailConnector` (+1688 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **502 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `AppState` connect `ADDED Requirements` to `D-004 — Every effectful action goes through `gate()``, `artifact_loader.rs`, `config.rs`, `mod.rs`, `.put`, `content.d.ts`, `AppState`, `D-006 — Identity is not authority`, `Requirements`, `Requirements`, `ADDED Requirements`, `decisions.md`, `properties`, `tests.rs`, `Ok`, `Proposal: Define core runtime schemas`, `Tasks: Implement selected-thread email preview slice`, `D-051 — The agent-OS canon (AD-001..153) is decomposed into a dependency-edged change sequence; the stale later-changes placeholders are superseded or subsumed`, `HeaderMap`, `mod.rs`?**
  _High betweenness centrality (0.037) - this node is a cross-community bridge._
- **Why does `ArtifactRegistry` connect `.put` to `add_column_if_missing`, `properties`, `Design: Gate action API`, `Ok`, `ADDED Requirements`, `StoreError`, `Tasks: Implement selected-thread email preview slice`, `ActionId`?**
  _High betweenness centrality (0.018) - this node is a cross-community bridge._
- **Why does `TaskGrant` connect `D-006 — Identity is not authority` to `decisions.md`, `README.md`, `config.rs`, `D-051 — The agent-OS canon (AD-001..153) is decomposed into a dependency-edged change sequence; the stale later-changes placeholders are superseded or subsumed`, `client.rs`, `ADDED Requirements`, `Requirements`, `HeaderMap`, `mod.rs`, `architecture.md`?**
  _High betweenness centrality (0.015) - this node is a cross-community bridge._
- **Are the 67 inferred relationships involving `handle_owner_update()` (e.g. with `activation_with_mutated_payload_is_denied()` and `approved_artifact_activates_into_registry_and_overlay()`) actually correct?**
  _`handle_owner_update()` has 67 INFERRED edges - model-reasoned connections that need verification._
- **Are the 65 inferred relationships involving `owner_update()` (e.g. with `activation_with_mutated_payload_is_denied()` and `approve_callback_update()`) actually correct?**
  _`owner_update()` has 65 INFERRED edges - model-reasoned connections that need verification._
- **Are the 47 inferred relationships involving `test_state_with_telegram()` (e.g. with `activation_with_mutated_payload_is_denied()` and `approved_artifact_activates_into_registry_and_overlay()`) actually correct?**
  _`test_state_with_telegram()` has 47 INFERRED edges - model-reasoned connections that need verification._
- **What connects `autoresearch.sh script`, `TelegramReplyPayload`, `ReadThreadPayload` to the rest of the system?**
  _1693 weakly-connected nodes found - possible documentation gaps or missing edges._