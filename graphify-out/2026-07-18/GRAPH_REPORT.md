# Graph Report - implement-standing-rules  (2026-07-18)

## Corpus Check
- 524 files · ~411,331 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 7161 nodes · 10089 edges · 1480 communities (540 shown, 940 thin omitted)
- Extraction: 87% EXTRACTED · 13% INFERRED · 0% AMBIGUOUS · INFERRED: 1275 edges (avg confidence: 0.8)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `9a2752bc`
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
- mod.rs
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
- D-019 — Implement minimal slice first, not full agent OS
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
- D-016 — Capability packs are candidate profiles, not live authority
- D-017 — Personas grant no authority
- D-018 — Routes are declarative artifacts, not kernel code
- Json
- Result
- State
- StatusCode
- String
- Value
- Vec
- String
- JoinHandle
- Option
- Response
- D-027 — Multi-provider model gateway with per-provider auth mode
- D-028 — Canonical artifact format is YAML+serde; digests are `sha256:` over canonical JSON
- ArtifactId
- CapabilityPack
- Error
- F
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
- template
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
- Box
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
- head
- Value
- D-034 — `email.create_draft` is the one canonical action id; the qualified PRD §10.2 spelling is dropped
- Result
- Option
- Result
- String
- Timestamp
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
- Requirement: Digest/brief format MUST remain a learnable default
- Result
- String
- opsx-apply.md
- opsx-propose.md
- SKILL.md
- SKILL.md
- SKILL.md
- opsx-apply.md
- opsx-archive.md
- Requirement: Seed guidance MUST keep negative constraints in eval probes
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
- Requirement: Worker result recording MUST be receipt-keyed and fail-closed (D-083)
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
- String
- Ulid
- ArtifactId
- Option
- Self
- String
- Timestamp
- Ulid
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
- failure_surfacing.rs
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
- Value
- Arc
- HeaderMap
- Json
- Result
- Option
- Result
- State
- StatusCode
- String
- String
- Vec
- Ulid
- Value
- Option
- Result
- Self
- String
- Timestamp
- Ulid
- Vec
- Option
- GmailConnector
- Option
- HandlerFuture
- TelegramConnector
- Store
- TelegramConnector
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
- opsx-archive.md
- opsx-propose.md
- SKILL.md
- SKILL.md
- SKILL.md
- opsx-apply.md
- opsx-archive.md
- HashMap
- Option
- Self
- Value
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
- Result
- Self
- String
- Vec
- Display
- Error
- Formatter
- HashMap
- Result
- Self
- String
- Connection
- Result
- String
- Transaction
- Vec
- Vec
- Arc
- ArtifactRef
- Display
- Option
- PathBuf
- String
- HashSet
- String
- Ulid
- Vec
- String
- Item
- Mutex
- Option
- Result
- String
- Ulid
- Vec
- Path
- PathBuf
- Store
- String
- HashSet
- Path
- PathBuf
- Result
- Store
- String
- HeaderMap
- Json
- Self
- String
- Vec
- Result
- String
- Result
- Arc
- HashMap
- Result
- State
- StatusCode
- Duration
- Result
- String
- Option
- Result
- ArtifactRef
- Option
- Result
- Ulid
- Store
- String
- Result
- ArtifactRef
- Connection
- Option
- Result
- String
- Ulid
- Vec
- ArtifactRef
- Option
- Result
- String
- Ulid
- Vec
- Arc
- Client
- Arc
- ArtifactRef
- AtomicBool
- Connection
- Error
- Mutex
- Option
- Path
- Ulid
- Value
- Vec
- Value
- Connection
- Option
- Result
- String
- Ulid
- Vec
- test_hooks.rs
- Connection
- Path
- Result
- Store
- String
- Vec
- Error
- TelegramConnector
- Ulid
- MockServer
- Digest
- Ulid
- ArtifactId
- String
- Vec
- Error
- From
- Mutex
- Option
- Response
- Result
- Self
- String
- Value
- Vec
- GmailConnector
- Option
- MockServer
- Value
- Response
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
- AsRef
- TryFrom
- Option
- Store
- Ulid
- ArtifactRef
- Option
- Result
- Ulid
- Vec
- ArtifactRef
- Option
- String
- Ulid
- Vec
- SocketAddr
- ArtifactId
- Connector
- Option
- Vec
- ArtifactRef
- AsRef
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
- TryFrom
- Ulid
- Vec
- PathBuf
- String
- TelegramConnector
- Arc
- HeaderMap
- Result
- Result
- Ulid
- Json
- Result
- ArtifactRef
- Store
- String
- Ulid
- Box
- Option
- Result
- String
- Value
- Option
- Result
- Value
- Box
- Result
- Ulid
- F
- Option
- R
- Result
- String
- Ulid
- Vec
- Ulid
- Option
- Result
- Briefcase
- GmailConnector
- Option
- Ulid
- Vec
- State
- StatusCode
- String
- ArtifactRef
- Value
- Vec
- ArtifactRef
- Mutex
- Option
- Path
- Result
- Self
- Arc
- Digest
- Option
- HandlerFuture
- Option
- String
- Value
- Vec
- Aes256Gcm
- ArtifactRef
- AtomicBool
- Digest
- Error
- PathBuf
- Result
- Self
- String
- Vec
- BTreeMap
- Result
- String
- Value
- Vec
- Option
- Result
- Store
- String
- Display
- Formatter
- Option
- Ulid
- BTreeMap
- IntoIterator
- Item
- Option
- Self
- String
- Ulid
- Vec
- Result
- Result
- Store
- Digest
- String
- String
- Vec
- Digest
- Digest
- Path
- Result
- Digest
- Digest
- Digest
- Digest
- Digest
- Digest
- Digest
- HashMap
- Into
- Option
- Result
- Self
- Store
- String
- Ulid
- WorkflowManifest
- ArtifactRef
- ArtifactId
- Option
- Result
- String
- Vec
- Store
- HashSet
- Path
- Result
- Store
- String
- Ulid
- Vec
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
- Ulid
- Option
- Result
- String
- Value
- Vec
- Option
- ResponseTemplate
- Result
- String
- Value
- mod.rs
- Result
- String
- opsx-apply.md
- opsx-archive.md
- HashSet
- Option
- Result
- Store
- String
- Path
- opsx-propose.md
- SKILL.md
- SKILL.md
- SKILL.md
- Ulid
- Vec
- Vec
- Option
- Result
- Vec
- Option
- PathBuf
- Store
- String
- HashSet
- Path
- PathBuf
- Result
- Store
- String
- Ulid
- Vec
- Connection
- F
- HashSet
- Into
- Option
- Result
- S
- Self
- String
- Ulid
- Vec
- Option
- Store
- Ulid
- ArtifactRef
- Connection
- Option
- Result
- String
- Ulid
- Vec
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
- String
- Vec
- Ulid
- ArtifactRef
- Digest
- Path
- PathBuf
- PersonaElement
- Result
- Store
- Ulid
- Vec
- ArtifactId
- String
- MockServer
- Option
- Result
- Option
- Result
- Ulid
- Vec
- GmailConnector
- MockServer
- Ulid
- Value
- Value
- ArtifactRef
- Connection
- Result
- Ulid
- Error
- Option
- Result
- Option
- GmailConnector
- Option
- PathBuf
- Store
- TelegramConnector
- Briefcase
- Option
- String
- MockServer
- Store
- String
- Vec
- String
- Digest
- Option
- Result
- String
- Ulid
- Vec
- Store
- Ulid
- WorkflowManifest
- Result
- Self
- Store
- String
- Ulid
- Digest
- Store
- Digest
- Store
- String
- ArtifactRef
- Briefcase
- Connection
- Digest
- Duration
- Option
- Result
- Store
- Ulid
- Vec
- Digest
- From
- Option
- Result
- Self
- Store
- Connection
- Option
- Result
- Ulid
- Vec
- String
- Result
- Store
- Vec
- Connection
- Option
- Result
- Row
- String
- Ulid
- Vec
- ArtifactRef
- Option
- Result
- Ulid
- Option
- Self
- String
- Ulid
- Option
- Self
- String
- Ulid
- Vec
- String
- Vec
- ArtifactRef
- Option
- Self
- String
- Vec
- Connection
- Digest
- Result
- Transaction
- Connection
- Digest
- Error
- Result
- Store
- String
- Transaction
- Ulid
- Vec
- Option
- Result
- Row
- Store
- String
- Ulid
- Vec
- Connection
- Result
- Store
- Transaction
- Ulid
- Ulid
- ArtifactRef
- Briefcase
- Digest
- Store
- Ulid
- Vec
- Store
- Ulid
- WorkflowManifest
- ArtifactRef
- BTreeMap
- Digest
- Display
- Formatter
- From
- HashMap
- HashSet
- Into
- IntoIterator
- Item
- Option
- Result
- Self
- String
- Ulid
- Vec
- ArtifactId
- Digest
- String
- Vec

## God Nodes (most connected - your core abstractions)
1. `handle_owner_update()` - 97 edges
2. `owner_update()` - 84 edges
3. `test_state()` - 79 edges
4. `StoreError` - 76 edges
5. `AppState` - 75 edges
6. `test_state_with_telegram()` - 69 edges
7. `ActionId` - 61 edges
8. `gate()` - 54 edges
9. `TaskGrant` - 49 edges
10. `digest_of_bytes()` - 43 edges

## Surprising Connections (you probably didn't know these)
- `converge_owner_accepted_dependencies()` --calls--> `dependency_fingerprint_allows()`  [INFERRED]
  crates/openspine-kernel/src/overlay_convergence.rs → crates/openspine-kernel/src/store/learned_artifacts.rs
- `plan_proposal_budget_exhaustion_persists_no_request()` --calls--> `test_state()`  [INFERRED]
  crates/openspine-kernel/src/pipeline/tests/plan.rs → crates/openspine-kernel/src/test_support.rs
- `task_rejects_plaintext_title_and_provenance_references()` --calls--> `task()`  [INFERRED]
  crates/openspine-schemas/src/task.rs → crates/openspine-kernel/src/store/task_board_tests.rs
- `task_rejects_unsupported_schema_version()` --calls--> `task()`  [INFERRED]
  crates/openspine-schemas/src/task.rs → crates/openspine-kernel/src/store/task_board_tests.rs
- `task_round_trips_through_serde()` --calls--> `task()`  [INFERRED]
  crates/openspine-schemas/src/task.rs → crates/openspine-kernel/src/store/task_board_tests.rs

## Import Cycles
- 2-file cycle: `crates/openspine-kernel/src/overlay_eval_gate/judge.rs -> crates/openspine-kernel/src/overlay_eval_gate/mod.rs -> crates/openspine-kernel/src/overlay_eval_gate/judge.rs`
- 2-file cycle: `crates/openspine-kernel/src/api/handler_registry.rs -> crates/openspine-kernel/src/pipeline/mod.rs -> crates/openspine-kernel/src/api/handler_registry.rs`
- 3-file cycle: `crates/openspine-kernel/src/api/actions.rs -> crates/openspine-kernel/src/pipeline/mod.rs -> crates/openspine-kernel/src/api/handler_registry.rs -> crates/openspine-kernel/src/api/actions.rs`
- 3-file cycle: `crates/openspine-kernel/src/api/actions.rs -> crates/openspine-kernel/src/store/standing_rules.rs -> crates/openspine-kernel/src/workflow.rs -> crates/openspine-kernel/src/api/actions.rs`
- 3-file cycle: `crates/openspine-kernel/src/pipeline/mod.rs -> crates/openspine-kernel/src/telegram.rs -> crates/openspine-kernel/src/workflow.rs -> crates/openspine-kernel/src/pipeline/mod.rs`
- 4-file cycle: `crates/openspine-kernel/src/api/actions.rs -> crates/openspine-kernel/src/pipeline/mod.rs -> crates/openspine-kernel/src/telegram.rs -> crates/openspine-kernel/src/workflow.rs -> crates/openspine-kernel/src/api/actions.rs`
- 5-file cycle: `crates/openspine-kernel/src/api/actions.rs -> crates/openspine-kernel/src/store/standing_rules.rs -> crates/openspine-kernel/src/workflow.rs -> crates/openspine-kernel/src/pipeline/mod.rs -> crates/openspine-kernel/src/api/handler_registry.rs -> crates/openspine-kernel/src/api/actions.rs`

## Communities (1480 total, 940 thin omitted)

### Community 0 - "README.md"
Cohesion: 0.08
Nodes (24): P, filter_within_scope(), class_digest(), Store, storage(), Store, AdvisorObjection, complete_declarations_round_trip_for_all_five_types() (+16 more)

### Community 1 - ".new"
Cohesion: 0.16
Nodes (8): verify_update(), configured_owner_text_message_is_verified(), invalid_candidate_bot_token_is_rejected_without_replacing_live_bot(), missing_sender_is_ignored(), non_text_update_from_owner_is_ignored(), owner_message_in_a_group_chat_is_ignored_not_routed(), unknown_telegram_user_is_ignored_not_routed(), update()

### Community 2 - "event.rs"
Cohesion: 0.09
Nodes (13): egress_declarations(), id(), Vec, action_id_qualifier_is_part_of_identity(), action_id_serializes_as_bare_string(), ActionCatalog, ActionEgressDeclaration, ActionRequest (+5 more)

### Community 3 - "handle_owner_update"
Cohesion: 0.06
Nodes (50): authenticate(), bearer_token(), get_status(), internal_error(), router(), Arc, ArtifactRef, Display (+42 more)

### Community 4 - ".default"
Cohesion: 0.05
Nodes (42): Purpose, Requirement: ActionCatalog MUST enumerate every trusted-path carve-out around gate(), Requirement: Approval-required decisions MUST not execute immediately, Requirement: Every effectful action MUST pass through gate(), Requirement: Gate decisions MUST be auditable, Requirement: Gate decisions MUST use task grant precedence, Requirement: Gate MUST verify authenticated grant caveat chains offline, Requirement: Grant limits MUST be enforced at runtime (+34 more)

### Community 5 - "GmailConnector"
Cohesion: 0.12
Nodes (36): accepted_digest_tamper_invalidates(), base_namespace_is_not_treated_as_learned(), dangling_learned_route_is_orphaned_and_excluded(), insert_agent(), insert_pack(), insert_route(), insert_workflow(), learned() (+28 more)

### Community 6 - "artifact_loader.rs"
Cohesion: 0.12
Nodes (16): Arc, ArtifactRef, AtomicBool, Connection, Digest, Error, Mutex, Option (+8 more)

### Community 7 - "config.rs"
Cohesion: 0.09
Nodes (38): committed_usage_count(), pending_token_consumed_at(), reserved_usage_count(), Option, Store, standing_rule_delivery_unknown_finalizes_live_reservation(), standing_rule_effective_allow_audit_failure_cancels_reservation(), standing_rule_fired_path_audit_failure_rearms_token_once() (+30 more)

### Community 8 - "mod.rs"
Cohesion: 0.05
Nodes (37): Purpose, Requirement: A mutated approved plan MUST be refused at the gate, Requirement: Approval audit MUST avoid plaintext private payloads, Requirement: Approval MUST bind only what the owner was shown, Requirement: Approvals MUST expire, Requirement: Draft creation MUST remain approval-required, Requirement: Draft creation MUST require digest-bound approval, Requirement: Final email send MUST remain denied (+29 more)

### Community 9 - "telegram.rs"
Cohesion: 0.13
Nodes (33): activation_with_mutated_payload_is_denied(), approve_callback_update(), approved_artifact_activates_into_registry_and_overlay(), model_swap_ceremony_switches_real_generate_provider(), mount_send_message_ok(), MockServer, TelegramConnector, Ulid (+25 more)

### Community 10 - ".put"
Cohesion: 0.06
Nodes (30): nerve-subscribers Specification, Purpose, Requirement: Advisor interjections are structured legibility objections, Requirement: Interjections ride the archived event-bus substrate, Requirement: Manifest limits and production screener dispatch are kernel-owned, Requirement: Nerve declaration schema, Requirement: Proactivity is a budgeted lane, Requirement: Registration validates advisee scope (+22 more)

### Community 11 - "ProposedArtifact"
Cohesion: 0.15
Nodes (22): counterparty_deferral_text_is_canonical(), grant_with_thread(), no_thread_id_resolves_to_master(), owner_escalation_message(), owner_message_carries_action_and_reason_code(), resolve_by_thread_id_returns_bound_grant(), resolve_grant_for_thread(), route_escalation() (+14 more)

### Community 12 - "client.rs"
Cohesion: 0.38
Nodes (8): approve_callback(), learned_route(), nomination_owner_tap_persists_nominated_status_and_audit(), nomination_rejects_persona_kind_before_authority_path(), nomination_requires_explicit_depersonalized_assertion(), telegram_ok(), ArtifactNominatePayload, dispatch_artifact_nominate()

### Community 13 - "policy.rs"
Cohesion: 0.17
Nodes (26): classified_empty_output_channel_denial(), commission(), commission_is_receipt_idempotent(), commission_receipt_binding_rejects_different_parent_or_request(), commissioning_persists_briefcase_without_board_row(), gate_request(), minimal_briefcase(), narrowed_gate_worker() (+18 more)

### Community 14 - "actions.rs"
Cohesion: 0.10
Nodes (41): canonical_json(), bindings_valid(), Caveat, caveat_bytes(), chain_structurally_valid(), ChainStep, compute_mac_hex(), compute_tip() (+33 more)

### Community 15 - "ADDED Requirements"
Cohesion: 0.13
Nodes (18): build_raw_reply_message(), extract_body_text(), header_value(), parse_thread(), bounded_json_response(), CachedToken, GmailConnector, extract_email_address() (+10 more)

### Community 16 - "Requirements"
Cohesion: 0.15
Nodes (8): Ok, CanonicalValue, CanonicalValue<'a>, Digest, digest_round_trips_through_serde(), HasherWriter<'a>, InvalidDigest, Write

### Community 17 - "content.d.ts"
Cohesion: 0.14
Nodes (24): artifact_key_bytes(), artifact_key_round_trips_bytes(), Config, ConfigError, default_kernel_bind(), example_configs_parse_against_the_real_schema(), gmail_client_secret(), gmail_refresh_token() (+16 more)

### Community 19 - "action.rs"
Cohesion: 0.20
Nodes (22): initialization_transaction_failure_rolls_back_then_retry_succeeds(), mount_unused_provider(), provider_pointed_at(), startup_migrates_old_token_and_legacy_offset_before_first_poll(), startup_preserves_offset_when_vault_token_matches_persisted(), startup_reconciles_vault_token_when_persisted_bot_id_differs(), startup_retries_getme_on_transient_failure(), assert_poll_offset() (+14 more)

### Community 20 - "StoreError"
Cohesion: 0.15
Nodes (19): AccountRole, ActorHint, ChannelTrust, Connector, DataClassification, EventEnvelope, EventType, InteractionMode (+11 more)

### Community 21 - "post_action"
Cohesion: 0.15
Nodes (18): Option, Result, String, Store, ensure_schema(), epoch_nanos_to_timestamp(), PendingScheduleCtx, ArtifactRef (+10 more)

### Community 22 - "ApprovalRecord"
Cohesion: 0.07
Nodes (29): ADDED Requirements, nerve-subscribers Specification, Requirement: Advisor interjections are structured legibility objections, Requirement: Interjections ride the archived event-bus substrate, Requirement: Manifest limits and production screener dispatch are kernel-owned, Requirement: Nerve declaration schema, Requirement: Proactivity is a budgeted lane, Requirement: Registration validates advisee scope (+21 more)

### Community 23 - "ActionId"
Cohesion: 0.05
Nodes (63): BTreeSet, apply_top_up(), apply_top_up_for_grant(), apply_top_up_for_grant_atomic(), BriefcaseKernelError, email_address_hash(), grant_view(), pack_for_pipeline() (+55 more)

### Community 24 - "AppState"
Cohesion: 0.11
Nodes (13): action_request_consume_is_single_use(), action_request_round_trips_by_id(), approval_round_trips_by_action_request_id(), find_task_grant_by_token_rejects_the_raw_hash_value(), most_recent_approval_wins_when_multiple_exist_for_one_request(), opening_a_pre_existing_db_without_the_used_column_is_migrated_in_place(), persisted_grant_json_contains_no_task_token(), sample_action_request() (+5 more)

### Community 25 - "AppState"
Cohesion: 0.21
Nodes (28): ActionRequestBody, ActionResponseBody, cleanup_pre_effect_reservations(), dispatch_lyra_preview(), dispatch_read_selected_thread(), DispatchError, FailureSurface, guard_connector_dispatch() (+20 more)

### Community 26 - ".sweep_expired_grants"
Cohesion: 0.16
Nodes (25): email_read_selected_thread_rejects_expired_token(), email_read_selected_thread_rejects_foreign_grant(), email_read_selected_thread_rejects_malformed_payload(), email_read_selected_thread_rejects_second_use(), email_read_selected_thread_returns_thread_via_mocked_gmail(), gmail_connector(), mint_grant_with_selection_token(), mount_gmail_thread_endpoint() (+17 more)

### Community 27 - "Lifecycle"
Cohesion: 0.14
Nodes (19): BuildEnvelopeFn, GrantBindingFn, PreflightFn, RouteGuardFn, RouteResolution, email_preview_lane(), LaneSpec, owner_control_lane() (+11 more)

### Community 28 - "Requirements"
Cohesion: 0.14
Nodes (15): EvalRow, digest(), fractional_timestamp_orders_after_exact_second(), insert_and_query_by_artifact_returns_ordered_rows(), latest_eval_verdict_returns_newest_for_artifact(), query_by_verdict_filters_across_artifacts(), verdict(), verdict_vocabulary_is_open() (+7 more)

### Community 29 - "OpenSpine Agent-OS Design Log"
Cohesion: 0.21
Nodes (3): AuditKind, AuditKindError, EventSubscriptionFilter

### Community 30 - "ArtifactRef"
Cohesion: 0.07
Nodes (27): ADDED Requirements, MODIFIED Requirements, Requirement: ActionCatalog MUST enumerate every trusted-path carve-out around gate(), Requirement: Approval-required decisions MUST not execute immediately, Requirement: Every effectful action MUST pass through gate(), Requirement: Gate decisions MUST be auditable, Requirement: Grant limits MUST be enforced at runtime, Requirement: Kernel-origin actions MUST route through gate() with a KernelOrigin marker (+19 more)

### Community 31 - "ADDED Requirements"
Cohesion: 0.13
Nodes (26): handle_owner_update(), Result, connector_failure_batches_and_is_audited(), counter_persistence_failure_is_durably_batched_as_resource(), immediate_failure_cannot_enter_digest_lane(), digest_detail_corrupt_blob_surfaces_resource_without_leak(), digest_detail_immediate_success_outcome_audit_failure_is_delivery_unknown_and_no_retry(), digest_detail_missing_blob_surfaces_resource_without_leak() (+18 more)

### Community 32 - "Requirements"
Cohesion: 0.12
Nodes (20): consult_and_reserve_atomic_budget_saturates_after_max_uses(), consult_and_reserve_cancel_leaves_headroom_unchanged(), consult_and_reserve_is_atomic_wrt_version_reactivation(), manifest(), manifest_validate_rejects_non_positive_windows(), owner_revoke_action_removes_rule_from_live_consultation(), reservation_identity(), Option (+12 more)

### Community 35 - "sandbox.rs"
Cohesion: 0.08
Nodes (25): ADDED Requirements, Requirement: Authority-sensitive changes MUST be explicitly marked, Requirement: Decision-log consistency MUST be preserved, Requirement: Every change MUST classify its affected layer, Requirement: OpenSpec archive MUST preserve rationale, Requirement: OpenSpec development process MUST define its purpose, Requirement: OpenSpec MUST remain separate from OpenSpine runtime authority, Requirement: PRD-derived work MUST be split into implementation slices (+17 more)

### Community 38 - "ADDED Requirements"
Cohesion: 0.19
Nodes (17): ActionBody, ActionOutcome, approval_required_is_ok_not_err(), counterparty_denial_preserves_only_canonical_deferral(), deny_decision_is_ok_not_err(), generate_sends_bearer_auth(), generate_sends_untrusted_context_in_body(), GenerateBody (+9 more)

### Community 39 - "Requirements"
Cohesion: 0.09
Nodes (12): resolve_text(), retry_backoff(), retry_due_notifications(), run_retry_loop(), Store, DeadLetterState, detail_insert_columns(), DetailReceipt (+4 more)

### Community 40 - "Requirements"
Cohesion: 0.13
Nodes (8): I, Iterator, built_in_web_egress_endpoints(), Connector, ConnectorRegistry, EgressRegistrationError, GmailConnector, TelegramConnector

### Community 41 - "Requirements"
Cohesion: 0.17
Nodes (19): AntiPattern, count_bullets(), count_occurrences(), every_anti_pattern_has_a_failing_sample(), has(), probe_apology_theater(), probe_deferential_double_asking(), probe_faked_intimacy() (+11 more)

### Community 42 - "Requirements"
Cohesion: 0.18
Nodes (10): Command, docker_driver_args_are_correct_and_secret_free(), DockerDriver, process_driver_allows_external_communication_with_explicit_opt_in(), process_driver_clears_env_and_sets_only_two_vars(), process_driver_never_refuses_owner_control_lane(), process_driver_refuses_external_communication_without_opt_in(), ProcessDriver (+2 more)

### Community 43 - "properties"
Cohesion: 0.08
Nodes (25): Purpose, Requirement: Authority-sensitive changes MUST be explicitly marked, Requirement: Completed OpenSpec changes MUST be archived, Requirement: Decision-log consistency MUST be preserved, Requirement: Each OpenSpec change MUST state affected layer, Requirement: OpenSpec artifacts MUST NOT be treated as live runtime authority, Requirement: OpenSpec development process MUST define its purpose, Requirement: PRD-derived work MUST be split into implementation slices (+17 more)

### Community 44 - "ADDED Requirements"
Cohesion: 0.08
Nodes (25): Purpose, Requirement: Installing or promoting a version MUST atomically retire lower versions (AD-041), Requirement: Mined skills require AD-110 promotion review before the shelf, Requirement: `skill.context` is a gated kernel action returning an untrusted envelope (AD-040/AD-042), Requirement: Skill provenance gates the install/update ceremony, Requirement: Skill selection is a read-only matcher that injects, never installs, Requirement: Skills are a versioned artifact class shaping competence only, Requirement: The owner promotion tap MUST be an authenticated, durable decision (AD-041/AD-110) (+17 more)

### Community 45 - "digest.rs"
Cohesion: 0.26
Nodes (22): highest_active_prunes_stale_loaded_lower_version(), learned_template_row(), missing_highest_blob_fails_closed_no_rollback(), on_disk_artifact_without_active_proposal_is_excluded(), orphan_active_yaml_without_db_row_is_quarantined(), overlay_template_yaml(), reconfirmed_legacy_template_stays_live_across_restart(), active_v2_survives_stale_approved_v1_recovery() (+14 more)

### Community 46 - "Digest"
Cohesion: 0.08
Nodes (25): Purpose, Requirement: Correlated task slices are bounded and anchored, Requirement: Deadline and reminder timers use the normal pipeline, Requirement: Master receives bounded task slices, Requirement: Scheduled grants use applicable least-privilege artifacts, Requirement: Tasks are durable kernel objects, Requirement: Timer dispatch is idempotent and classified, Requirement: Timer dispatch validates owner and dependencies (+17 more)

### Community 47 - "Design: OpenSpine development process"
Cohesion: 0.26
Nodes (37): a_deny_route_is_never_composed(), approval_required_overrides_plain_allow(), compose_accepts_unwired_known_id(), compose_rejects_unknown_action_id_with_structured_error(), email_grant_excludes_inbox_wide_read_matching_prd_12_2(), explicit_deny_overrides_allow(), main_assistant_grant_never_inherits_email_drafter_authority(), no_candidate_allow_means_action_is_not_granted() (+29 more)

### Community 48 - "ADDED Requirements"
Cohesion: 0.25
Nodes (16): handle_artifact_nominate(), handle_artifact_propose(), handle_artifact_revoke(), handle_lyra_preview(), handle_plan_propose(), handle_read_selected_thread(), handle_setup_workflow_start(), handle_skill_context() (+8 more)

### Community 49 - "ADDED Requirements"
Cohesion: 0.08
Nodes (24): ADDED Requirements, Kernel task board, Requirement: Correlated task slices are bounded and anchored, Requirement: Deadline and reminder timers use the normal pipeline, Requirement: Master receives bounded task slices, Requirement: Scheduled grants use applicable least-privilege artifacts, Requirement: Tasks are durable kernel objects, Requirement: Timer dispatch is idempotent and classified (+16 more)

### Community 50 - "mod.rs"
Cohesion: 0.15
Nodes (18): GoldenSet, GoldenSetCase, GoldenSetCaseKind, GoldenSetCaseResult, GoldenSetRunResult, GoldenSetValidationError, ModelRole, ModelSwapManifest (+10 more)

### Community 51 - "ADDED Requirements"
Cohesion: 0.28
Nodes (14): bounded_text(), load_checkpoint(), relay_one(), RelayOutcome, resolve_relay_context(), Option, Result, String (+6 more)

### Community 52 - "ADDED Requirements"
Cohesion: 0.08
Nodes (24): ADDED Requirements, Requirement: Installing or promoting a version MUST atomically retire lower versions (AD-041), Requirement: Mined skills require AD-110 promotion review before the shelf, Requirement: `skill.context` is a gated kernel action returning an untrusted envelope (AD-040/AD-042), Requirement: Skill provenance gates the install/update ceremony, Requirement: Skill selection is a read-only matcher that injects, never installs, Requirement: Skills are a versioned artifact class shaping competence only, Requirement: The owner promotion tap MUST be an authenticated, durable decision (AD-041/AD-110) (+16 more)

### Community 53 - "Requirements"
Cohesion: 0.08
Nodes (24): model-swap-ceremony Specification, Purpose, Requirement: Activation MUST use a serialized provenance-bound staged protocol, Requirement: Golden sets MUST be bounded and role-bound, Requirement: Model swaps MUST be evidence-bearing AD-142 proposals, Requirement: Provider configuration changes MUST NOT bypass the ceremony, Requirement: Restart MUST require symmetric latest ceremony provenance, Requirements (+16 more)

### Community 54 - "Requirements"
Cohesion: 0.22
Nodes (17): persona_overlay_dir(), repair_persona_file(), seed_definitions(), seed_if_missing(), stage_persona(), a_committed_row_with_a_missing_file_self_heals_without_new_provenance(), a_corrupt_existing_file_is_repaired_to_the_row_digest(), audit_failure_rolls_back_learned_row_and_seeded_receipt() (+9 more)

### Community 55 - "Requirements"
Cohesion: 0.08
Nodes (24): Purpose, Requirement: Attachments MUST be denied in the preview slice, Requirement: Email content MUST be treated as untrusted data, Requirement: Email read MUST be selected-thread only, Requirement: Email workflow MUST require a trusted selected-thread token, Requirement: Model calls with private email context MUST use model gateway, Requirement: Preview output MUST be reviewable by the owner, Requirement: Preview slice MUST NOT send email (+16 more)

### Community 56 - "owner_event"
Cohesion: 0.25
Nodes (22): benign_body(), containment_gate_denies_denied_action_regardless_of_malicious_skill(), make_skill(), malicious_body(), matcher_can_inject_but_never_install(), mined_provenance_lands_pending_and_review_denies_malicious_body(), mined_provenance_promotes_when_review_passes(), owner_can_reject_pending_mined_skill() (+14 more)

### Community 57 - ".default"
Cohesion: 0.15
Nodes (19): build_owner_envelope(), CallbackQueryUpdate, parse_approve_callback(), parse_approve_plan_callback(), parse_bind_command(), parse_digest_detail_command(), parse_digest_namespace(), parse_draft_command() (+11 more)

### Community 58 - "Runtime schema groups"
Cohesion: 0.08
Nodes (23): ADDED Requirements, Model swap ceremony Specification, Requirement: Activation MUST use a serialized provenance-bound staged protocol, Requirement: Golden sets MUST be bounded and role-bound, Requirement: Model swaps MUST be evidence-bearing AD-142 proposals, Requirement: Provider configuration changes MUST NOT bypass the ceremony, Requirement: Restart MUST require symmetric latest ceremony provenance, Scenario: Approved Base swap changes the gateway selection (+15 more)

### Community 59 - "ADDED Requirements"
Cohesion: 0.08
Nodes (23): ADDED Requirements, artifact-lifecycle Specification, Requirement: Anti-pattern probes MUST fail on violating output and pass on clean output, Requirement: Digest/brief format MUST remain a learnable default, Requirement: Every AD-081/AD-083 anti-pattern MUST have an eval probe, Requirement: Persona artifacts MUST never enter kernel authority, Requirement: Personality seed artifacts MUST load as overlay learned artifacts with provenance, Requirement: Personality seed MUST NOT be kernel-baked base fixtures (+15 more)

### Community 60 - "ADDED Requirements"
Cohesion: 0.08
Nodes (23): day-2-operations Specification, Purpose, Requirement: Audit I/O failure handling, Requirement: Boot clock-regression detection, Requirement: One-set snapshot and restore, Requirement: Same-conversation serialization, Requirement: Telegram-first first-run sequence, Requirement: Versioned schema migrations (+15 more)

### Community 61 - "ADDED Requirements"
Cohesion: 0.11
Nodes (10): AgentManifest, CapabilityPack, ModelSwapManifest, ParsedProposal, PersonaElement, Policy, PromptTemplate, StandingRuleManifest (+2 more)

### Community 62 - "ADDED Requirements"
Cohesion: 0.09
Nodes (22): ADDED Requirements, Requirement: Dark-window defaults are durable and replay-safe, Requirement: Drift requires re-review, Requirement: Quota and rate are independent atomic boundaries, Requirement: Remaining budget is visible, Requirement: Standing rules are reviewed composition inputs, Scenario: Allow default grants one waiver, Scenario: Concurrent admission cannot overspend (+14 more)

### Community 63 - "Requirements"
Cohesion: 0.50
Nodes (3): truncate_for_telegram(), truncate_with_notice(), dispatch_plan_preview()

### Community 64 - "scripts"
Cohesion: 0.21
Nodes (16): Future, Output, Pin, Send, emit_preflight_failure(), email_build_envelope(), email_preflight(), EventInputs (+8 more)

### Community 65 - "compose_authority"
Cohesion: 0.13
Nodes (13): evaluate(), JudgeDenial, Digest, Result, String, evaluate(), ReplayDenial, GateDenial (+5 more)

### Community 66 - "ConnectorRegistry"
Cohesion: 0.15
Nodes (9): deserialize_schema_version(), provenance_round_trips_with_tag(), ref_of(), slice_round_trips_and_rejects_unknown_fields(), task_rejects_plaintext_title_and_provenance_references(), task_rejects_unsupported_schema_version(), task_round_trips_through_serde(), WorkerId (+1 more)

### Community 67 - "identity.rs"
Cohesion: 0.09
Nodes (22): ADDED Requirements, Day-2 Operations, Requirement: Audit I/O failure handling, Requirement: Boot clock-regression detection, Requirement: One-set snapshot and restore, Requirement: Same-conversation serialization, Requirement: Telegram-first first-run sequence, Requirement: Versioned schema migrations (+14 more)

### Community 68 - "properties"
Cohesion: 0.09
Nodes (22): ADDED Requirements, Artifact lifecycle overlay delta, MODIFIED Requirements, Requirement: Activated artifacts MUST survive a kernel restart, Requirement: Base and overlay namespace collisions MUST refuse replacement, Requirement: Compatibility MUST fail closed for dangling learned references, Requirement: Learned artifacts MUST carry durable exchange provenance, Requirement: Legacy overlay migration is discovery/quarantine only (+14 more)

### Community 69 - "tests.rs"
Cohesion: 0.09
Nodes (22): Purpose, Requirement: Connector secrets MUST be captured outside shell/model context, Requirement: Connectors MUST resolve credentials at call time, Requirement: Intake and rotation mode transitions MUST use normal gate authority, Requirement: Paired Gmail credentials MUST be staged and promoted atomically, Requirement: Pending captures MUST be bound and fail closed, Requirement: Secret intake outcomes MUST be metadata-only and auditable, Requirement: Secret values MUST be encrypted at rest and rotatable (+14 more)

### Community 70 - "Proposal: Define OpenSpine development process"
Cohesion: 0.09
Nodes (22): Adversarial review (from AdversarialAgentLit research, 2026-07-07), Authority growth (settled), Base/overlay & updates (settled), Blindspot resolutions (2026-07-07, owner-approved: recommendations Q1-Q7 adopted), Core axes (settled), Delegation & containment (settled), Egress & connectors (settled), Game-AI patterns (from GameAiPatterns research, 2026-07-07) (+14 more)

### Community 71 - "MODIFIED Requirements"
Cohesion: 0.19
Nodes (5): secret_introduction_and_rotation_are_decryptable(), SecretStore, SecretStoreError, seed_is_idempotent_and_slot_validation_is_strict(), truncated_or_corrupted_ciphertext_fails_closed()

### Community 72 - "properties"
Cohesion: 0.09
Nodes (21): ADDED Requirements, Requirement: Action requests and gate decisions MUST be typed, Requirement: Approval records MUST bind reviewed payloads and targets, Requirement: Audit schemas MUST reference private payloads by encrypted or hash refs, Requirement: Event envelopes MUST include source authenticity fields, Requirement: Identity schemas MUST NOT grant runtime authority, Requirement: OpenSpine core runtime objects MUST have explicit schemas, Requirement: Route resolution schemas MUST represent ambiguity (+13 more)

### Community 73 - "create_approved_draft"
Cohesion: 0.09
Nodes (21): ADDED Requirements, Requirement: Connector secrets MUST be captured outside shell/model context, Requirement: Connectors MUST resolve credentials at call time, Requirement: Intake and rotation mode transitions MUST use normal gate authority, Requirement: Paired Gmail credentials MUST be staged and promoted atomically, Requirement: Pending captures MUST be bound and fail closed, Requirement: Secret intake outcomes MUST be metadata-only and auditable, Requirement: Secret values MUST be encrypted at rest and rotatable (+13 more)

### Community 74 - ".put"
Cohesion: 0.20
Nodes (17): all_slice_categories_are_owner_scoped(), anchored_slice_includes_not_yet_due_focal_task_and_honors_limit_one(), insert_task_rejects_prepopulated_timer_ids(), master_slice_is_bounded_and_excludes_task_detail(), master_slice_is_owner_scoped(), ref_of(), schedule_rolls_back_after_timer_insert_failure(), scheduling_rejects_mismatched_or_terminal_task() (+9 more)

### Community 75 - "SKILL.md"
Cohesion: 0.16
Nodes (6): lifecycle_name(), lineage_from_json(), lineage_to_json(), parse_lifecycle(), ProposedArtifact, Store

### Community 76 - "Tasks: Harden approval and budgets"
Cohesion: 0.10
Nodes (20): ADDED Requirements, Purpose, Requirement: Allowed-action dispatch MUST resolve through a handler registry, Requirement: Connectors MUST be registered through a connector registry, Requirement: Post-approval resolution MUST route through a registry with a draft-creation default, Requirement: Proposable artifact kinds MUST have a single source of truth, Requirement: Unknown action ids MUST be denied at gate with a structured reason, Requirement: Unknown action ids MUST fail fast at composition (+12 more)

### Community 77 - "properties"
Cohesion: 0.10
Nodes (20): failure-surfacing Specification, Purpose, Requirement: Artifact-backed dead letters, Requirement: Connector counters, Requirement: Delivery-unknown crash semantics, Requirement: Direct authenticated bad-request surfacing, Requirement: Durable effect receipts, Requirement: Failure taxonomy routing (+12 more)

### Community 78 - "explore.md"
Cohesion: 0.10
Nodes (20): kernel-registries Specification, Purpose, Requirement: Allowed-action dispatch MUST resolve through a handler registry, Requirement: Connectors MUST be registered through a connector registry, Requirement: Post-approval resolution MUST route through a registry with a draft-creation default, Requirement: Proposable artifact kinds MUST have a single source of truth, Requirement: Unknown action ids MUST be denied at gate with a structured reason, Requirement: Unknown action ids MUST fail fast at composition (+12 more)

### Community 79 - "OpenSpine kernel↔shell HTTP contract"
Cohesion: 0.19
Nodes (4): ArtifactLineage, LineageParent, root_has_generation_zero_and_no_parents(), root_round_trips_through_json()

### Community 80 - "Ok"
Cohesion: 0.17
Nodes (15): capture(), audit_failure_rolls_back_live_credential(), audit_failure_rolls_back_paired_promotion(), gmail_paired_intake_stages_first_half_then_promotes_on_second(), gmail_paired_intake_works_in_reverse_order(), action_for(), arm(), CaptureOutcome (+7 more)

### Community 81 - "Proposal: Define core runtime schemas"
Cohesion: 0.15
Nodes (22): build_state(), build_state_with_store(), repo_lyra_dir(), test_state_with_gmail_and_telegram(), test_state_with_store(), gated_step_persisted_pending_recovers_without_redispatch(), failed_callback_ack_does_not_abort_approval_and_is_durable(), a_double_tap_on_approve_creates_only_one_gmail_draft() (+14 more)

### Community 82 - "Proposal: Implement authority composition"
Cohesion: 0.26
Nodes (9): Bot, Arc, Mutex, Result, Self, String, TelegramBotState, TelegramConnector (+1 more)

### Community 83 - "Proposal: Implement digest-bound draft approval"
Cohesion: 0.18
Nodes (14): materialize_missing(), parsed(), seed_yaml(), SeedError, write_seed_files(), all_seeds_parse_and_validate_as_state_machines(), digest(), email_draft_seed_declares_digest_bound_approval_state() (+6 more)

### Community 84 - "Proposal: Implement gate action API"
Cohesion: 0.18
Nodes (16): AgentLimits, AgentManifest, main_assistant_agent(), main_assistant_denies_broad_email_access(), MemoryScope, ModelPolicy, OutputChannels, Persistence (+8 more)

### Community 85 - "Proposal: Implement selected-thread email preview slice"
Cohesion: 0.10
Nodes (19): ADDED Requirements, Purpose, Requirement: Activated artifacts MUST survive a kernel restart, Requirement: Activation MUST require digest-bound owner approval, Requirement: Artifact id and version MUST be unique across fixtures, overlay, and pending proposals, Requirement: Only active artifacts MUST participate in authority composition, Requirement: Prompt templates MUST NOT be proposable at runtime, Requirement: Proposed artifacts MUST be schema-validated before persistence (+11 more)

### Community 86 - "Tasks: Implement selected-thread email preview slice"
Cohesion: 0.13
Nodes (12): admission_below_cap_is_allowed_and_does_not_breach(), alert_fires_drains_and_rearms_on_new_day(), breach_is_recorded_exactly_once_per_day(), cap_i64(), concurrent_reservations_never_cross_cap(), connector_reserve_is_atomic_capped_and_day_scoped(), fresh_day_with_zero_cap_is_a_hard_denial(), inflight_alert_is_rearmed_for_crash_recovery() (+4 more)

### Community 87 - "Proposal: Implement Telegram owner control slice"
Cohesion: 0.07
Nodes (23): counterparty_resolves_identity_but_no_principal(), handle_owner_bind(), IdentityResolver, IdentityResolver<'a>, owner_verified_path_resolves_owner_principal_and_relationship(), unknown_resolves_to_relationship_unknown_confidence_0_and_no_write(), deny_unknown_fields_rejects_capability_pack_id(), EntityType (+15 more)

### Community 88 - "Proposal: Backfill implemented capability specs"
Cohesion: 0.20
Nodes (23): approval_required_action_stops_before_dispatch(), counterparty_denial_returns_deferral_routes_and_audits(), counterparty_escalation_failure_is_not_reported_as_success(), email_read_inbox_is_denied_for_owner_control_grant(), host_filesystem_read_and_write_are_denied_for_owner_control_grant(), model_swap_propose_reaches_http_gate_and_audits_failure(), network_raw_egress_is_denied_for_owner_control_grant(), post_action() (+15 more)

### Community 89 - "Proposal: Harden approval and budgets"
Cohesion: 0.06
Nodes (55): ApprovalStepInputs, ArtifactRef, bool, Digest, digest_inputs(), EntryBindingInputs, EntryTransitionInputs, GatedDepartureInputs (+47 more)

### Community 90 - "Proposal: Implement artifact lifecycle slice"
Cohesion: 0.10
Nodes (19): ADDED Requirements, Failure surfacing, Requirement: Artifact-backed dead letters, Requirement: Connector counters, Requirement: Delivery-unknown crash semantics, Requirement: Direct authenticated bad-request surfacing, Requirement: Durable effect receipts, Requirement: Failure taxonomy routing (+11 more)

### Community 91 - "Tasks: Implement artifact lifecycle slice"
Cohesion: 0.06
Nodes (60): deny_limit_exceeded(), GenerateRequestBody, GenerateResponseBody, post_model_generate(), template_id_for_agent(), admit_spend(), breach_message(), counted_model_generate() (+52 more)

### Community 92 - "Agent-OS change sequence (2026-07-07, AD canon)"
Cohesion: 0.10
Nodes (19): Purpose, Requirement: Approval-required MUST override plain allow, Requirement: Authority composition MUST be deny-by-default, Requirement: Authority widening MUST require explicit approval, Requirement: Connector and account role MUST NOT grant authority by themselves, Requirement: Explicit deny MUST override allow, Requirement: Identity MUST NOT grant authority by itself, Requirement: Main assistant grant MUST NOT inherit specialist workflow authority (+11 more)

### Community 93 - "AuditMeta"
Cohesion: 0.20
Nodes (13): advisee_scope_of(), atomic_ingestion_emits_no_signal_for_clean_text(), atomic_ingestion_emits_signal_on_marker(), dispatch_revokes_registration_when_limits_disappear(), dummy_ref(), manifest(), register_screener(), screener_ignores_unknown_marker_signal() (+5 more)

### Community 94 - "AuditEvent"
Cohesion: 0.14
Nodes (13): causal_containment_through_skill_context_dispatch(), make_skill(), malicious_body(), String, is_unique_constraint_violation(), persist_promotion_decision_conn(), sample_skill(), Skill (+5 more)

### Community 95 - "Tasks: Define core runtime schemas"
Cohesion: 0.27
Nodes (18): cyclic_approval_replay_uses_each_visit_binding(), declared_step_tier_routes_through_gateway_map(), digest(), entering_approval_state_without_request_is_rejected(), entry_binds_request_and_departure_requires_exact_match(), entry_rejects_request_whose_action_differs_from_state(), expired_completed_approval_rehydrates_from_immutable_step_proof(), failed_reserved_step_does_not_reset_reconstructed_target() (+10 more)

### Community 96 - "Tasks: Define OpenSpine development process"
Cohesion: 0.24
Nodes (14): oversized_skill_id_is_rejected_before_preview_rendering(), promotion_refuses_when_higher_installed_exists(), promotion_retires_lower_installed_version_atomically(), record_preview(), skill_with(), trusted_install_refuses_equal_version_reinstall(), trusted_install_refuses_lower_version_when_higher_exists_any_state(), unsupported_schema_version_is_rejected_fail_closed() (+6 more)

### Community 97 - "Tasks: Implement digest-bound draft approval"
Cohesion: 0.25
Nodes (16): a_failed_token_refresh_surfaces_as_an_error(), a_non_404_api_error_is_not_treated_as_missing(), connector(), fetch_thread_extracts_text_and_skips_attachments(), gmail_token_refresh_only_within_skew_window(), gmail_token_refreshes_near_expiry(), mount_token_endpoint(), rotated_vault_credentials_bypass_live_access_token_cache() (+8 more)

### Community 98 - "Tasks: Implement Telegram owner control slice"
Cohesion: 0.19
Nodes (9): CompatibilityStatus, dependency_fingerprint_allows(), LearnedArtifact, nomination_name(), NominationStatus, parse_nomination(), parse_status(), status_name() (+1 more)

### Community 99 - "artifact_activation_tests.rs"
Cohesion: 0.11
Nodes (18): ADDED Requirements, Requirement: Approval-required MUST override plain allow, Requirement: Authority composition MUST be deny-by-default, Requirement: Authority widening MUST require explicit approval, Requirement: Connector and account role MUST NOT grant authority by themselves, Requirement: Explicit deny MUST override allow, Requirement: Identity MUST NOT grant authority by itself, Requirement: Main assistant grant MUST NOT inherit specialist workflow authority (+10 more)

### Community 100 - "Design: Authority composition"
Cohesion: 0.22
Nodes (8): PostApprovalHandler, handle_draft_approval_callback(), handle_plan_approval_callback(), resolve_approved_plan(), finalize_nomination(), NominationAssertion, resolve_post_approval_handler(), notify_owner_best_effort()

### Community 101 - "Tasks: Backfill implemented capability specs"
Cohesion: 0.11
Nodes (18): ADDED Requirements, MODIFIED Requirements, Requirement: Approval audit MUST avoid plaintext private payloads, Requirement: Approval MUST bind only what the owner was shown, Requirement: Draft creation MUST require digest-bound approval, Requirement: The kernel MUST re-derive digests from artifact-store bytes at approval-effect time, Scenario: Approval is recorded, Scenario: Audit records the re-derived digest (+10 more)

### Community 102 - "ADDED Requirements"
Cohesion: 0.11
Nodes (18): ADDED Requirements, MODIFIED Requirements, Requirement: A mutated approved plan MUST be refused at the gate, Requirement: Plan approval MUST bind the complete ordered step-list digest, Requirement: Plan steps MUST bind exact execution identity, Requirement: The kernel MUST re-derive digests from artifact-store bytes at approval-effect time, Scenario: Arguments change while summary remains unchanged, Scenario: Data-handling step participates in the digest (+10 more)

### Community 103 - "ADDED Requirements"
Cohesion: 0.11
Nodes (18): audit-artifact-store Specification, Purpose, Requirement: Artifact blobs MUST be encrypted and content-addressed by plaintext digest, Requirement: Audit append MUST assign per-aggregate sequence under the store lock, Requirement: Reading an artifact MUST re-verify its digest after decryption, Requirement: Task tokens MUST be stored hashed, never in plaintext, Requirement: The audit log MUST be append-only and hash-chained, Requirement: The kernel MUST verify the audit chain on startup and refuse to start if broken (+10 more)

### Community 104 - "OpenSpine conventions"
Cohesion: 0.25
Nodes (17): authority_granted_audit_is_atomic_with_grant_persist(), deadline_timer_reaches_routed_granted_and_worker_gate(), fires_task_timer_and_reaches_worker_gate(), linked_fired_event_after_task_cancelled_ack_skips_without_grant(), linked_fired_event_after_task_done_ack_skips_without_grant(), recovery_refuses_receiptless_handed_off_redispatch(), ref_of(), reminder_timer_uses_precise_route_and_worker_gate() (+9 more)

### Community 105 - "openspine-decision-log.md"
Cohesion: 0.31
Nodes (16): active_rule(), allowed_pending_is_recoverable_until_consumed(), claimed_fired_pending_is_surfaced_once_not_redispatched(), deny_default_never_dispatches_and_is_terminal(), fired_allow_token_is_digest_bound_and_one_use(), fired_allow_token_rejects_different_fingerprint(), owner_resolution_before_fire_controls_claim(), payload() (+8 more)

### Community 107 - "OpenSpine"
Cohesion: 0.12
Nodes (4): complete_u32(), divergent_inputs_fail_closed(), ledger_corruption_fails_closed(), ModelOutput

### Community 108 - "Gmail selected-thread email preview setup (Phase 2)"
Cohesion: 0.27
Nodes (17): allow_reply(), allow_result(), approval_required_on_primary_action_exits_ok_no_reply(), cmd_freeform(), cmd_propose(), cmd_setup(), cmd_status(), deny_on_model_generate_exits_ok() (+9 more)

### Community 109 - "Telegram owner-control setup (Phase 1)"
Cohesion: 0.11
Nodes (17): 1. `define-core-runtime-schemas`, 2. `implement-authority-composition`, 3. `implement-gate-action-api`, 4. `implement-telegram-owner-control-slice`, 5. `implement-selected-thread-email-preview-slice`, Authority-sensitive changes, Context, Design goals (+9 more)

### Community 110 - "Tasks: Implement authority composition"
Cohesion: 0.40
Nodes (16): artifact_ref(), email_route(), exact_deny_route_wins_over_allow_route(), gmail_connector_authenticated_alone_does_not_match_the_selected_thread_route(), higher_priority_route_wins_over_lower_priority(), matches(), no_matching_route_is_denied_not_ambiguous(), no_relationship_match_denies_the_route() (+8 more)

### Community 111 - "Design: Digest-bound draft approval"
Cohesion: 0.11
Nodes (17): ADDED Requirements, Requirement: Attachments MUST be denied in the preview slice, Requirement: Email content MUST be treated as untrusted data, Requirement: Email read MUST be selected-thread only, Requirement: Email workflow MUST require a trusted selected-thread token, Requirement: Model calls with private email context MUST use model gateway, Requirement: Preview output MUST be reviewable by the owner, Requirement: Preview slice MUST NOT send email (+9 more)

### Community 112 - "Design: Gate action API"
Cohesion: 0.11
Nodes (17): ADDED Requirements, Core runtime schemas, Requirement: Commissioned workers MUST receive a briefcase, not the board (D-085), Requirement: Master MUST commission workers and relay results as bus events, Requirement: Worker grants MUST have no effective output channel (reply chokepoint), Requirement: Worker result recording MUST be receipt-keyed and fail-closed (D-083), Requirement: Worker sub-grants MUST be offline-verifiable caveat-chain attenuations, Scenario: Briefcase scoped to the worker (`commissioning_persists_briefcase_without_board_row`) (+9 more)

### Community 113 - "Tasks: Implement gate action API"
Cohesion: 0.13
Nodes (7): ConsumerError, IdempotentConsumer, LedgerEntry, PersistedConsumerState, Store, Store, WorkerRelayClaim

### Community 114 - "Design: Selected-thread email preview slice"
Cohesion: 0.18
Nodes (8): append(), consumer_id_is_bound_to_filter(), failed_handler_does_not_advance_checkpoint(), filtered_replay_is_idempotent(), kind_filter_preserves_global_order(), persisted_checkpoint_survives_reload(), replay_after_watermark_skips_earlier_rows(), unique_ids_and_per_aggregate_sequences()

### Community 115 - "Design: Telegram owner control slice"
Cohesion: 0.11
Nodes (17): escalation-and-refusal Specification, Purpose, Requirement: Counterparty-facing gate denials at the worker action chokepoint MUST surface only the canonical deferral plus an owner-routed escalation, Requirement: Escalation routing MUST be deterministic kernel machinery, Requirement: No policy or rule text MUST cross the worker-facing chokepoint as human-facing content, Requirement: Thread↔grant binding MUST be kernel-owned and dormant until a thread-capable channel ships, Requirements, Scenario: Allowed action does not escalate or defer (+9 more)

### Community 116 - "Requirement: Selection tokens MUST be single-use, expiring, and scope-fixed"
Cohesion: 0.12
Nodes (16): ADDED Requirements, Purpose, Requirement: Lane specifications MUST be compiled-in kernel data, Requirement: Per-flow variation MUST be lane data interpreted by one driver, Requirement: The audited event envelope MUST be emitted only after verification succeeds, Requirement: The driver MUST NOT invoke gate(), Requirement: The kernel pipeline MUST be a typed stage sequence the driver executes, Scenario: A lane cannot skip a stage (+8 more)

### Community 118 - "Kernel foundation"
Cohesion: 0.12
Nodes (16): ADDED Requirements, Requirement: Counterparty-facing gate denials at the worker action chokepoint MUST surface only the canonical deferral plus an owner-routed escalation, Requirement: Escalation routing MUST be deterministic kernel machinery, Requirement: No policy or rule text MUST cross the worker-facing chokepoint as human-facing content, Requirement: Thread↔grant binding MUST be kernel-owned and dormant until a thread-capable channel ships, Scenario: Allowed action does not escalate or defer, Scenario: Counterparty ApprovalRequired also returns deferral, routes to owner, and audits, Scenario: Counterparty deferral is exactly the canonical constant (+8 more)

### Community 119 - "attrs"
Cohesion: 0.12
Nodes (16): briefcase-packing Specification, Purpose, Requirement: Depth limits packed content, Requirement: Every task is packed before worker spawn, Requirement: Kernel packs every task deterministically, Requirement: Top-ups are kernel-mediated and gate-visible, Requirement: Visibility classes are enforced structurally, Requirements (+8 more)

### Community 120 - "Design: Harden approval and budgets"
Cohesion: 0.62
Nodes (6): complete_task_and_wake(), dispatch_task_wake(), reattempt_handed_off(), recover_timer_dispatches(), run_task_deadline_consumer(), TimerDispatchOutcome

### Community 121 - "Failure surfacing & operations"
Cohesion: 0.12
Nodes (16): pipeline-driver Specification, Purpose, Requirement: Lane specifications MUST be compiled-in kernel data, Requirement: Per-flow variation MUST be lane data interpreted by one driver, Requirement: The audited event envelope MUST be emitted only after verification succeeds, Requirement: The driver MUST NOT invoke gate(), Requirement: The kernel pipeline MUST be a typed stage sequence the driver executes, Requirements (+8 more)

### Community 122 - "D-008 — Deterministic routing decides authority; agentic routing decides strategy"
Cohesion: 0.29
Nodes (15): denied_read_thread_stops_without_drafting(), Draft, draft_reply(), empty_draft_skips_preview_without_error(), format_thread_for_model(), format_thread_for_model_includes_all_fields(), full_flow_reads_drafts_and_previews(), no_selection_tokens_is_an_error() (+7 more)

### Community 123 - "D-001 — Lyra is a runtime/substrate, not a single agent"
Cohesion: 0.26
Nodes (14): breaker_transitions_closed_open_half_open_closed(), dropped_permit_reopens_breaker_instead_of_wedging_half_open(), failures_outside_window_expire(), interleaved_successes_do_not_mask_windowed_failures(), invalid_webhook_signature_does_not_poison_valid_key(), now(), open_breaker_blocks_even_after_cooldown_until_probe_recorded(), rate_limit_refills_after_backoff_interval() (+6 more)

### Community 124 - "mod.rs"
Cohesion: 0.12
Nodes (15): ADDED Requirements, Briefcase Packing, Requirement: Depth limits packed content, Requirement: Every task is packed before worker spawn, Requirement: Kernel packs every task deterministically, Requirement: Top-ups are kernel-mediated and gate-visible, Requirement: Visibility classes are enforced structurally, Scenario: Identical task shape and snapshot (+7 more)

### Community 125 - "D-042 — Reply recipient is kernel-derived, never shell-supplied: newest non-owner sender, matched against a configured mailbox address"
Cohesion: 0.12
Nodes (15): ADDED Requirements, Durable workflow replay, Ratified decisions, Requirement: Failures are terminal and replayable, Requirement: Gated and approval identity is bound, Requirement: Private payloads do not leak, Requirement: Timers have atomic durable transitions, Requirement: Workflow steps replay by exact durable handle (+7 more)

### Community 126 - "D-002 — First usable UX should include an owner control channel"
Cohesion: 0.12
Nodes (15): durable-workflow-replay Specification, Purpose, Requirement: Failures are terminal and replayable, Requirement: Gated and approval identity is bound, Requirement: Private payloads do not leak, Requirement: Timers have atomic durable transitions, Requirement: Workflow steps replay by exact durable handle, Requirements (+7 more)

### Community 127 - "D-003 — Gmail is a guarded workflow, not the whole product"
Cohesion: 0.12
Nodes (15): lineage-and-eval-store Specification, Purpose, Requirement: Artifacts MUST carry a generation/lineage model distinct from content version, Requirement: Eval-verdict vocabulary MUST remain open and fitness/evidence optional, Requirement: Eval verdicts MUST land in an indexed table, not the audit chain, Requirement: Unknown lineage MUST NOT be rewritten as root, Requirements, Scenario: A row with no lineage loads as None (+7 more)

### Community 128 - "D-004 — Every effectful action goes through `gate()`"
Cohesion: 0.28
Nodes (14): claim_and_redispatch(), decode_missing_payload_ref_propagates_error(), decode_pending_payload(), fired_connector_pre_effect_failure_rearms_then_retries_once(), recover_unredriven_pending(), recovery_surfaces_claimed_and_propagates_missing_none_payload(), ArtifactRef, Option (+6 more)

### Community 129 - "D-005 — Private-data shell must be contained"
Cohesion: 0.10
Nodes (22): ArtifactProposePayload, String, activate_approved_artifact(), Result, AppState, bounded_preview(), content_diff_summary(), mint_reconfirm_grant() (+14 more)

### Community 130 - "D-009 — External content is data, not instruction"
Cohesion: 0.48
Nodes (11): fixture(), mixed_model_swap_recovery_keeps_active_version_and_merge_preserves_it(), persist_active_provenance(), persist_proposal_state(), post_commit_pending_crash_restores_matching_reviewed_overlay(), pre_commit_pending_crash_discards_pending_and_keeps_old_disk(), swap_yaml(), swap_yaml_version() (+3 more)

### Community 131 - "D-021 — Email domain is broader than Gmail"
Cohesion: 0.15
Nodes (5): runtime_clock_observation_survives_restart_and_preserves_maximum(), runtime_timer_driver_observation_is_durable(), legacy_digest_summary_is_sanitized_idempotently(), versioned_migrations_atomicity_rollback(), versioned_migrations_up_down_up()

### Community 132 - "D-022 — Agent-owned inbox is distinct from owner mailbox access"
Cohesion: 0.13
Nodes (14): ADDED Requirements, Purpose, Requirement: Artifact blobs MUST be encrypted and content-addressed by plaintext digest, Requirement: Reading an artifact MUST re-verify its digest after decryption, Requirement: Task tokens MUST be stored hashed, never in plaintext, Requirement: The audit log MUST be append-only and hash-chained, Requirement: The kernel MUST verify the audit chain on startup and refuse to start if broken, Scenario: A row is appended (+6 more)

### Community 134 - "D-024 — OpenSpec is the development/change-management layer, not the runtime"
Cohesion: 0.28
Nodes (12): admits_more_than_three_tiny_items(), detail_pages(), detail_pages_reconstruct_every_byte_on_utf8_boundaries(), drains_aggregate_across_pages_without_duplicates(), handle_command(), handle_detail_command(), item(), long_detail_is_bounded_in_summary_and_full_via_ref() (+4 more)

### Community 135 - "D-026 — Shell containment via a `SandboxDriver` trait (Process dev-only / Docker)"
Cohesion: 0.19
Nodes (8): Sha256, digest_from_hash(), digest_of_bytes(), digest_of_bytes_hashes_raw_content_directly(), digest_of_is_a_pinned_golden_value(), HasherWriter, test_digest_matches_hash(), prune_non_highest_active_preserves_persona_typed_and_source_entries()

### Community 136 - "banner"
Cohesion: 0.14
Nodes (23): plan_preview_records_telegram_failure_counter_on_send_error(), plan_preview_records_telegram_success_counter(), plan_proposal_budget_exhaustion_persists_no_request(), plan_propose_approve_rederives_gate_and_resolves(), proposed_plan_fixture(), tampered_plan_artifact_is_refused_at_approval_callback(), test_state_with_telegram(), notify_owner_required() (+15 more)

### Community 137 - "super::Store"
Cohesion: 0.13
Nodes (9): Connection, Path, R, Result, Store, String, Vec, super::Store (+1 more)

### Community 138 - "add_column_if_missing"
Cohesion: 0.23
Nodes (11): Cli, commit_post_bind_clock(), grant_hmac_key(), main(), Option, PathBuf, Result, Store (+3 more)

### Community 139 - "Design: Backfill implemented capability specs"
Cohesion: 0.13
Nodes (14): ADDED Requirements, Purpose, Requirement: The Docker driver MUST provide no-public-egress networking, a read-only rootfs, and a non-root user, Requirement: The kernel MUST refuse external-communication events under the Process driver without explicit opt-in, Requirement: The kernel↔shell transport trust assumption MUST be documented, Requirement: The shell environment MUST contain only KERNEL_ENDPOINT and TASK_TOKEN, Scenario: A shell container is spawned in production, Scenario: A shell container is spawned under DockerDriver (+6 more)

### Community 140 - "Design: Artifact lifecycle slice"
Cohesion: 0.13
Nodes (14): ADDED Requirements, lineage-and-eval-store Specification, Requirement: Artifacts MUST carry a generation/lineage model distinct from content version, Requirement: Eval-verdict vocabulary MUST remain open and fitness/evidence optional, Requirement: Eval verdicts MUST land in an indexed table, not the audit chain, Requirement: Unknown lineage MUST NOT be rewritten as root, Scenario: A row with no lineage loads as None, Scenario: An open-vocabulary verdict is accepted (+6 more)

### Community 141 - "Delegation & containment"
Cohesion: 0.38
Nodes (3): map_slice_row(), Store, TaskSlice

### Community 142 - "Skills & workflows"
Cohesion: 0.32
Nodes (11): agent_manifests_round_trip(), artifacts_dir(), email_grant_pack_excludes_read_inbox_and_send(), every_fixture_file_is_covered_by_a_test(), global_policy_round_trips_and_denies_send(), owner_control_pack_round_trips(), owner_email_selected_thread_route_is_expressible_declaratively(), owner_telegram_route_is_expressible_declaratively() (+3 more)

### Community 143 - "Reflection & product surface"
Cohesion: 0.19
Nodes (13): call_with_connector(), call_with_connector_preflight(), call_with_connector_write(), confirmed_gmail_api_write_failure_is_connector_error(), gmail_malformed_success_response_is_delivery_unknown(), gmail_transport_write_is_delivery_unknown(), map_admission_error(), map_write_error() (+5 more)

### Community 144 - "package.json"
Cohesion: 0.13
Nodes (14): Acceptance Criteria, Affected layer, Authority sensitivity, Decision-log check, Dependencies, Goals, Non-goals, Out of Scope (+6 more)

### Community 145 - "D-008 — Deterministic routing decides authority; agentic routing decides strategy"
Cohesion: 0.18
Nodes (6): Identified, AgentManifest, CapabilityPack, Policy, PromptTemplate, WorkflowManifest

### Community 146 - "D-037 — Gmail OAuth via a plain refresh-token POST (no `oauth2` crate); `base64` promoted from transitive to direct dependency"
Cohesion: 0.13
Nodes (14): Acceptance Criteria, Affected layer, Authority sensitivity, Decision-log check, Dependencies, Goals, Non-goals, Out of Scope (+6 more)

### Community 147 - "model_swap.rs"
Cohesion: 0.13
Nodes (14): connector-reality Specification, Purpose, Requirement: Connector calls MUST be bounded, Requirement: Connector effects MUST have per-connector admission controls, Requirement: Gmail credentials MUST refresh before expiry, Requirement: Webhook admission MUST reject spoofed and replayed requests, Requirements, Scenario: Connector call exceeds timeout (+6 more)

### Community 148 - "D-040 — Pending (pre-approval) `ActionRequest`s are persisted in a new `action_requests` table"
Cohesion: 0.13
Nodes (14): Purpose, Requirement: The Docker driver MUST provide no-public-egress networking, a read-only rootfs, and a non-root user, Requirement: The kernel MUST refuse external-communication events under the Process driver without explicit opt-in, Requirement: The kernel↔shell transport trust assumption MUST be documented, Requirement: The shell environment MUST contain only KERNEL_ENDPOINT and TASK_TOKEN, Requirements, Scenario: A shell container is spawned in production, Scenario: A shell container is spawned under DockerDriver (+6 more)

### Community 149 - "D-041 — `email.create_draft`'s digest composition: payload = `{subject, body}`, target = `{thread_id, connector, account_role, recipients}`"
Cohesion: 0.13
Nodes (14): Purpose, Requirement: Main assistant task grant MUST be narrow, Requirement: Owner Telegram messages MUST normalize into event envelopes, Requirement: Telegram owner messages MUST be source verified, Requirement: Telegram owner route MUST resolve deterministically, Requirement: Telegram reply MUST use the owner channel only, Requirements, Scenario: Agent attempts reply to different chat (+6 more)

### Community 150 - "D-043 — `lyra.ui.preview` is extended (not duplicated) to propose the exact reviewed draft and attach the approval button"
Cohesion: 0.22
Nodes (9): empty_session_policy(), Constraints, Policy, SessionPolicy, allow_and_deny_lists_never_overlap_in_the_fixture(), AppliesTo, CapabilityPack, owner_control_basic_pack() (+1 more)

### Community 151 - "D-044 — Approved draft creation dispatches kernel-side; no new shell spawn"
Cohesion: 0.20
Nodes (7): committed_usage_count(), reservation_identity(), reserved_usage_count(), Option, Store, String, rule_status()

### Community 152 - "D-045 — WYSIWYS: a truncated preview refuses an approval button rather than splitting the message"
Cohesion: 0.31
Nodes (11): PromotionDenial, CeremonyError, CeremonyToken, install_mined_skill(), install_seed_skill(), install_skill_internal(), install_user_skill(), OwnerSkillDecision (+3 more)

### Community 153 - "D-046 — Grant budgets are enforced kernel-dispatch-side; the artifact budget counts only shell-initiated puts"
Cohesion: 0.14
Nodes (13): Artifacts and audit, Authority, Connectors and model calls, Context, Decision, Design: Core runtime schemas, Event and authenticity, Execution boundary (+5 more)

### Community 154 - "D-047 — Task tokens are hashed at rest; expired grants are swept"
Cohesion: 0.14
Nodes (13): ADDED Requirements, Requirement: Approval audit MUST avoid plaintext private payloads, Requirement: Draft creation MUST remain approval-required, Requirement: Draft creation MUST require digest-bound approval, Requirement: Final email send MUST remain denied, Requirement: Target mutation MUST invalidate approval, Scenario: Agent requests send after draft creation, Scenario: Approval is recorded (+5 more)

### Community 155 - "D-048 — `artifact.activate` is the single canonical activation action id; every runtime proposal requires uniform owner approval; prompt templates are excluded from proposable kinds"
Cohesion: 0.14
Nodes (13): ADDED Requirements, Requirement: Approval-required decisions MUST not execute immediately, Requirement: Every effectful action MUST pass through gate(), Requirement: Gate decisions MUST be auditable, Requirement: Gate decisions MUST use task grant precedence, Requirement: Unspecified actions MUST be denied, Scenario: Action appears in allowed and approval-required lists, Scenario: Action appears in allowed and denied lists (+5 more)

### Community 156 - "D-049 — Capability specs are backfilled for subsystems implemented inside earlier slices"
Cohesion: 0.14
Nodes (13): ADDED Requirements, Requirement: Main assistant task grant MUST be narrow, Requirement: Owner Telegram messages MUST normalize into event envelopes, Requirement: Telegram owner messages MUST be source verified, Requirement: Telegram owner route MUST resolve deterministically, Requirement: Telegram reply MUST use the owner channel only, Scenario: Agent attempts reply to different chat, Scenario: Configured owner sends message (+5 more)

### Community 157 - "D-050 — `max_model_calls` is enforced with an atomic upsert, not a count-then-compare"
Cohesion: 0.06
Nodes (69): approved_plan_digest_allows_through_gate(), argument_mutation_denied_when_summary_is_unchanged(), mutated_plan_after_approval_is_denied_at_gate(), allowed_action_returns_allow(), allowed_plus_approval_required_returns_approval_required(), allowed_plus_denied_returns_deny(), approval_for(), approval_required_action_does_not_execute() (+61 more)

### Community 158 - "D-006 — Identity is not authority"
Cohesion: 0.16
Nodes (14): load_base_registry(), load_registry(), parse_proposal(), generic_overlay_loader_excludes_persona_and_base_loader_rejects_fixture(), kind_table_excludes_personas(), kind_table_excludes_templates(), kind_table_round_trips_all_seven_kinds(), loads_every_real_fixture_without_error() (+6 more)

### Community 160 - "D-012 — Audit stores private payloads by encrypted/hash reference"
Cohesion: 0.14
Nodes (13): ADDED Requirements, Purpose, Requirement: Conversation state MUST store only role and artifact digest, Requirement: Private-context model calls MUST be constructed kernel-side, Requirement: Prompt templates MUST come from the kernel registry, never from shell input, Requirement: Provider credentials MUST never reach the shell, Requirement: Untrusted context MUST be wrapped with a per-call randomised delimiter, Scenario: A conversation turn is persisted (+5 more)

### Community 161 - "D-013 — Dynamic behavior easy; dynamic authority hard"
Cohesion: 0.26
Nodes (4): Drop, ConnectorCallError, ConnectorProbePermit, ConnectorRuntime

### Community 162 - "D-014 — Bootstrap/setup secrets bypass shell/model context"
Cohesion: 0.14
Nodes (13): model-gateway Specification, Purpose, Requirement: Conversation state MUST store only role and artifact digest, Requirement: Private-context model calls MUST be constructed kernel-side, Requirement: Prompt templates MUST come from the kernel registry, never from shell input, Requirement: Provider credentials MUST never reach the shell, Requirement: Untrusted context MUST be wrapped with a per-call randomised delimiter, Requirements (+5 more)

### Community 163 - "D-015 — Phase 1 should avoid final email send"
Cohesion: 0.14
Nodes (13): Purpose, Requirement: Approval-semantic transitions are digest-bound and replayable, Requirement: Gateway routing respects declared reasoning tiers, Requirement: Workflow manifests declare a reviewable state machine, Requirements, Scenario: Active provider swap remains visible, Scenario: High-tier step selects high provider, Scenario: Legacy manifest remains readable (+5 more)

### Community 164 - "D-016 — Capability packs are candidate profiles, not live authority"
Cohesion: 0.14
Nodes (13): dependencies, astro, @astrojs/starlight, sharp, name, scripts, astro, build (+5 more)

### Community 165 - "D-017 — Personas grant no authority"
Cohesion: 0.42
Nodes (13): owner_decide_promotion(), benign_body(), make_mined_skill(), malicious_body(), owner_approval_promotes_through_evaluator(), owner_approve_but_evaluator_denies_labels_decision_approve(), owner_decide_rejects_unknown_principal(), owner_rejection_keeps_skill_off_shelf() (+5 more)

### Community 166 - "D-018 — Routes are declarative artifacts, not kernel code"
Cohesion: 0.22
Nodes (11): ArtifactKindSpec, find_kind_spec(), is_proposable_kind(), overlay_subdir_for_kind(), rehydrate_source(), replace_keyed(), Error, HashMap (+3 more)

### Community 167 - "D-020 — Railway/Docker/VPS are deployment targets, not core architecture"
Cohesion: 0.44
Nodes (12): admission_returns_structure_only_after_budget_debit(), advisee_scope(), broader_scope_is_unregistrable_and_writes_no_row(), declaration(), five_ignored_reactions_retire_class_and_all_signals_persist(), nerve_tables_never_store_interjection_plaintext(), objection(), provenance() (+4 more)

### Community 168 - "D-025 — Rust/Tokio substrate: storage, audit chain, and secrets handling"
Cohesion: 0.42
Nodes (12): child_cannot_widen_parent_action(), child_cannot_widen_parent_expiry(), commission_spec(), direct_worker_egress_impossible(), mint_task_token(), mint_worker_grant(), minted_worker_has_empty_output_channels_despite_parent(), MintError (+4 more)

### Community 169 - "D-027 — Multi-provider model gateway with per-provider auth mode"
Cohesion: 0.42
Nodes (8): derived_lineage_round_trips_on_artifact_row(), inconsistent_generation_zero_with_parents_is_rejected(), inconsistent_positive_generation_without_parents_is_rejected(), lineage_is_distinct_from_version(), root_lineage_round_trips_on_artifact_row(), row_with(), stored_inconsistent_lineage_fails_closed_on_load(), unknown_lineage_is_none_not_root()

### Community 170 - "D-028 — Canonical artifact format is YAML+serde; digests are `sha256:` over canonical JSON"
Cohesion: 0.45
Nodes (12): add_column_if_missing(), apply_ad_hoc_migrations(), apply_single_migration_for_test(), apply_single_migration_inner(), apply_versioned_migrations(), latest_user_version(), read_user_version(), revert_versioned_migrations_for_test() (+4 more)

### Community 171 - "D-029 — Gmail OAuth scopes: `readonly` + `compose`, send hard-denied at the gate"
Cohesion: 0.24
Nodes (5): Result, Vec, Store, String, StandingRulePendingAction

### Community 172 - "openspine-decision-log.md"
Cohesion: 0.15
Nodes (12): Consequences, Consequences, D-079 — Overlay compatibility converges to a fixed point and base wins identity collisions, D-087 — Workflow state machines are declarative with digest-bound approval authorization, Decision, Decision, Decision Index, Lyra PRD Companion — Decisions Log (+4 more)

### Community 173 - "D-031 — Docker Compose is the first reference deployment target"
Cohesion: 0.29
Nodes (12): breaker_transitions_closed_open_half_open_closed(), invalid_webhook_signature_does_not_poison_valid_key(), now(), open_breaker_blocks_even_after_cooldown_until_probe_recorded(), rate_buckets_are_isolated_per_connector(), rate_limit_refills_after_backoff_interval(), rate_limit_when_empty_keeps_breaker_closed(), replayed_webhooks_are_rejected() (+4 more)

### Community 174 - "D-032 — Kernel↔shell transport is HTTP/JSON with a per-task bearer token"
Cohesion: 0.20
Nodes (4): BreakerState, CircuitBreaker, ConnectorRuntimeState, VecDeque

### Community 175 - "D-033 — Action identifiers are exact-match dotted strings; unverified senders are audited and ignored"
Cohesion: 0.24
Nodes (9): effect_defaults_to_allow_when_omitted(), owner_route(), round_trips_through_serde(), Route, route_can_be_a_deny_route(), RouteActorWhen, RouteEffect, RouteWhen (+1 more)

### Community 176 - "D-034 — `email.create_draft` is the one canonical action id; the qualified PRD §10.2 spelling is dropped"
Cohesion: 0.27
Nodes (10): step(), digest_is_order_argument_and_schema_sensitive(), digest_matches_serialized_plan_payload(), Plan, PlanApprovalQuestion, PlanStep, question_renders_every_digest_bound_field(), sample() (+2 more)

### Community 177 - "D-035 — Kernel advertises a separate `advertise_endpoint` from its `bind_addr`; no Unix-domain-socket transport for `ProcessDriver`"
Cohesion: 0.15
Nodes (12): 1. Extend `AuditEvent` — not a parallel envelope, 2. Schema: columns on `audit_log`, not a new events table, 3. Append path — sequence under the same lock as the insert, 4. Typed filter + ordered replay (read path), 5. Idempotent consumer — ack only after successful handling, 6. File layout (rebase-friendly), Alternatives considered, Approach (+4 more)

### Community 178 - "D-010 — Model calls with private context go through model gateway"
Cohesion: 0.15
Nodes (12): artifact-lifecycle Specification Delta, MODIFIED Requirements, Requirement: Authority-bearing proposals require overlay evaluation before approval, Requirement: Proposed artifacts MUST be schema-validated before persistence, Scenario: An unknown kind is rejected, Scenario: Generic lifecycle bypass is rejected, Scenario: Malformed YAML is rejected, Scenario: Missing model-swap evaluation blocks approval (+4 more)

### Community 179 - "D-019 — Implement minimal slice first, not full agent OS"
Cohesion: 0.15
Nodes (12): ADDED Requirements, Requirement: Approval-semantic transitions are digest-bound and replayable, Requirement: Gateway routing respects declared reasoning tiers, Requirement: Workflow manifests declare a reviewable state machine, Scenario: Active provider swap remains visible, Scenario: High-tier step selects high provider, Scenario: Legacy manifest remains readable, Scenario: Missing approval blocks departure (+4 more)

### Community 180 - "why-openspine.md"
Cohesion: 0.15
Nodes (12): ADDED Requirements, Requirement: Connector calls MUST be bounded, Requirement: Connector effects MUST have per-connector admission controls, Requirement: Gmail credentials MUST refresh before expiry, Requirement: Webhook admission MUST reject spoofed and replayed requests, Scenario: Connector call exceeds timeout, Scenario: Near-expiry token refreshes, Scenario: Open breaker blocks before effect (+4 more)

### Community 181 - "Q: Trace the full runtime call path for a single incoming Telegram message from an authorized owner, all the way through to a TaskGrant being persisted in the kernel's SQLite store. List the modules/files involved, in call order, and identify the 2-3 modules that are most structurally central to this flow."
Cohesion: 0.15
Nodes (13): Requirement: Base and overlay namespace collisions MUST refuse replacement, Requirement: Compatibility MUST fail closed for dangling learned references, Requirement: Every AD-081/AD-083 anti-pattern MUST have an eval probe, Requirement: Persona artifacts MUST never enter kernel authority, Requirement: Personality seed MUST NOT be kernel-baked base fixtures, Requirement: Prompt templates MUST NOT be proposable at runtime, Requirements, Scenario: A template proposal is rejected (+5 more)

### Community 182 - "Blindspot pass"
Cohesion: 0.15
Nodes (12): event-bus Specification, Purpose, Requirement: Bus events MUST carry unique IDs and per-aggregate sequence numbers, Requirement: Consumers MUST be idempotent and ack only after successful handling, Requirement: Consumers MUST subscribe via typed filters and ordered ledger replay, Requirement: The event bus MUST be the append-only audit ledger with no parallel store, Requirements, Scenario: Append is durable before consumer observation (+4 more)

### Community 183 - "Brainstorm and prototypes"
Cohesion: 0.24
Nodes (4): run_nerve_dispatcher(), screen_text(), screener_handler(), Store

### Community 184 - "Change quiz"
Cohesion: 0.15
Nodes (12): identity-store Specification, Purpose, Requirement: A Principal is a first-class, authority-free record and v1 enforces exactly one owner, Requirement: Identity binding MUST happen only via an audited, owner-approved path, Requirement: Identity resolution MUST be a read-only seam that never binds or mints principals, Requirements, Scenario: Binding attempt without owner context is rejected, Scenario: Idempotent bootstrap establishes exactly one owner (+4 more)

### Community 185 - "Implementation notes"
Cohesion: 0.35
Nodes (10): artifact_ref(), commission_and_record(), parent_grant(), test_briefcase(), worker_result_consumer_relays_through_parent_gated_reply(), worker_result_event_id_and_seq(), worker_result_relay_artifact_put_failure_stays_retryable(), worker_result_relay_is_idempotent_on_replay() (+2 more)

### Community 186 - "Implementation plan"
Cohesion: 0.15
Nodes (12): Purpose, Requirement: Email-draft seed gates departure on a digest-bound approval, Requirement: Minimal seed workflow set ships as overlay artifacts, Requirement: Seed workflows render Mermaid flowcharts, Requirements, Scenario: A seed renders every transition, Scenario: All four seeds parse and validate as state machines, Scenario: Departure without approval is rejected (+4 more)

### Community 187 - "Interview me"
Cohesion: 0.18
Nodes (4): ArtifactNamespace, ArtifactRef, can_transition(), Lifecycle

### Community 188 - "Pitch packager"
Cohesion: 0.35
Nodes (6): ArtifactRef, Option, Result, String, Ulid, Store

### Community 189 - "Reference hunt"
Cohesion: 0.17
Nodes (11): Goals, Non-goals, OpenSpec / OpenSpine boundary, OpenSpine / Lyra boundary, Proposal: Define OpenSpine development process, Proposed first implementation slices after this change, Risks, Scope (+3 more)

### Community 190 - "template"
Cohesion: 0.17
Nodes (11): ADDED Requirements, MODIFIED Requirements, Requirement: Completed OpenSpec changes MUST be archived, Requirement: OpenSpec artifacts MUST NOT be treated as live runtime authority, Requirement: Security-load-bearing subsystems MUST gain a capability spec in the change that implements them, Scenario: A change implements a new gated subsystem, Scenario: Change is complete, Scenario: Completed process change (+3 more)

### Community 191 - "architecture.md"
Cohesion: 0.29
Nodes (6): Debug, decode_hex(), signed_at_key_and_payload(), WebhookEnvelope, WebhookRejection, WebhookVerifier

### Community 192 - "decisions.md"
Cohesion: 0.33
Nodes (9): dispatch_polled_updates_for_test(), initialize_telegram_bot_id(), initialize_telegram_bot_id_until_ready(), is_already_processed(), resolve_telegram_offset(), resolve_telegram_offset_for_test(), dispatch_polled_updates(), poll_telegram_once_for_test() (+1 more)

### Community 193 - "quickstart.md"
Cohesion: 0.17
Nodes (11): 1. Carve-out enumeration as data (D-055.1), 2. KernelOrigin marker (D-055.2), 3. Selection-token validation into gate() (D-055.3), 4. Digest re-derivation (D-055.4), Alternatives considered, Approach, Design: Harden gate trusted paths, Key decisions (D-055) (+3 more)

### Community 194 - "roadmap.md"
Cohesion: 0.17
Nodes (11): MODIFIED Requirements, Requirement: Email workflow MUST require a trusted selected-thread token, Requirement: Selection tokens MUST be single-use, expiring, and scope-fixed, Scenario: Gate denies a missing or wrong-type token, Scenario: Gate validates the selection token for the read action, Scenario: Shell provides thread ID directly, Scenario: Token reused after consumption, Scenario: Token used after expiry (+3 more)

### Community 195 - "threat-model.md"
Cohesion: 0.17
Nodes (11): 1. Pure surface types — schemas/escalation.rs, 2. Integrated chokepoint — POST /v1/actions denial branch, 3. Thread_id fields — EventEnvelope + TaskGrant, 4. Thread↔grant binding resolver — kernel/escalation.rs, 5. Mandatory owner delivery from the API layer, Alternatives considered, Approach, Authority sensitivity (+3 more)

### Community 196 - "tsconfig.json"
Cohesion: 0.17
Nodes (11): ADDED Requirements, Requirement: Bus events MUST carry unique IDs and per-aggregate sequence numbers, Requirement: Consumers MUST be idempotent and ack only after successful handling, Requirement: Consumers MUST subscribe via typed filters and ordered ledger replay, Requirement: The event bus MUST be the append-only audit ledger with no parallel store, Scenario: Append is durable before consumer observation, Scenario: Double filtered replay is a pure no-op, Scenario: Failed handling does not advance the checkpoint (+3 more)

### Community 197 - "editUrl"
Cohesion: 0.17
Nodes (11): ADDED Requirements, Requirement: A Principal is a first-class, authority-free record and v1 enforces exactly one owner, Requirement: Identity binding MUST happen only via an audited, owner-approved path, Requirement: Identity resolution MUST be a read-only seam that never binds or mints principals, Scenario: Binding attempt without owner context is rejected, Scenario: Idempotent bootstrap establishes exactly one owner, Scenario: Owner asserts a binding successfully, Scenario: Owner resolves successfully (+3 more)

### Community 198 - "head"
Cohesion: 0.17
Nodes (11): ADDED Requirements, Requirement: Email-draft seed gates departure on a digest-bound approval, Requirement: Minimal seed workflow set ships as overlay artifacts, Requirement: Seed workflows render Mermaid flowcharts, Scenario: A seed renders every transition, Scenario: All four seeds parse and validate as state machines, Scenario: Departure without approval is rejected, Scenario: Materialization runs once per fresh install (+3 more)

### Community 199 - "pagefind"
Cohesion: 0.17
Nodes (11): egress-classes Specification, Purpose, Requirement: Capability packs MUST reference allowed egress classes, Requirement: The connector registry MUST type and protect egress endpoints, Requirement: The gate MUST enforce registry-rated egress classes, Requirements, Scenario: A conflicting registration cannot downgrade an endpoint, Scenario: Pack egress classes become live grant authority (+3 more)

### Community 200 - "AGENTS.md"
Cohesion: 0.17
Nodes (11): Purpose, Requirement: Configurable caps, Requirement: Global daily spend ledger, Requirement: Lane-aware breach boundary, Requirements, Scenario: Concurrent reservation cannot overspend, Scenario: Configured caps are enforced, Scenario: Counters persist by UTC day (+3 more)

### Community 201 - "AGENTS.md"
Cohesion: 0.38
Nodes (10): bind_email_identity(), email_counterparty_resolves_to_bound_identity_when_address_is_bound(), email_counterparty_stays_unresolved_when_address_is_unbound(), minimal_grant(), mint_topup_grant(), post_topup(), topup_granted_grant_mutates_briefcase_atomically(), topup_oversized_section_key_is_rejected_without_persistence() (+2 more)

### Community 202 - "proposal.rs"
Cohesion: 0.31
Nodes (8): SkillRow, find_live_skill_context_selection(), insert_skill_context_selection(), installed_skills_for_agent_and_pack(), live_skill_context_selections(), map_skill_row(), read_skill_row(), SkillContextSelection

### Community 203 - "autoresearch.sh"
Cohesion: 0.18
Nodes (10): Check for context, Ending Discovery, Guardrails, Handling Different Entry Points, OpenSpec Awareness, The Stance, What You Don't Have To Do, What You Might Do (+2 more)

### Community 204 - "CLAUDE.md"
Cohesion: 0.18
Nodes (10): Check for context, Ending Discovery, Guardrails, Handling Different Entry Points, OpenSpec Awareness, The Stance, What You Don't Have To Do, What You Might Do (+2 more)

### Community 207 - "check-claims.sh"
Cohesion: 0.18
Nodes (10): Check for context, Ending Discovery, Guardrails, Handling Different Entry Points, OpenSpec Awareness, The Stance, What You Don't Have To Do, What You Might Do (+2 more)

### Community 208 - "check-file-sizes.sh"
Cohesion: 0.18
Nodes (10): Check for context, Ending Discovery, Guardrails, Handling Different Entry Points, OpenSpec Awareness, The Stance, What You Don't Have To Do, What You Might Do (+2 more)

### Community 209 - "README.md"
Cohesion: 0.18
Nodes (10): 1. WYSIWYS (2a), 2. Enforce `max_model_calls` (2b), 3. Enforce `max_artifacts` (2c), 4. Hash task tokens at rest; sweep expired grants (2d), 5. Audit the trusted notification path (2e), 6. Operator docs (2f), 7. Spec deltas (2g), 8. Decision log (2h) (+2 more)

### Community 211 - "index.mdx"
Cohesion: 0.18
Nodes (10): ADDED Requirements, Requirement: Gate MUST verify authenticated grant caveat chains offline, Requirement: Shadow-mode grants MUST return a non-executable decision, Scenario: Action outside an action_allowlist caveat is not granted, Scenario: Dispatch does not execute on effect_suppressed, Scenario: Shadow allow becomes effect_suppressed, Scenario: Shadow deny remains deny, Scenario: Tampered or reordered caveats are rejected (+2 more)

### Community 213 - "apply.md"
Cohesion: 0.29
Nodes (3): DependencyWake, TimerDispatchRecord, TimerDispatchState

### Community 214 - "archive.md"
Cohesion: 0.29
Nodes (4): dependency_wake_requires_all_dependencies_and_unblocks_task(), ref_of(), Task, TaskProvenance

### Community 215 - "propose.md"
Cohesion: 0.18
Nodes (10): 1. gate crate — pure `gate()` changes, 2. ActionCatalog metadata, 3. Kernel wiring, 4. Digest re-derivation at approval-effect time, 5. Characterization tests — one per enumerated carve-out entry, 6. Threat-claims register rows (implementation task — NOT edited here), 7. Decision-log D-055 (implementation task — NOT edited here), 8. Docs (+2 more)

### Community 217 - "SKILL.md"
Cohesion: 0.33
Nodes (3): RequestError, candidate_error_is_invalid_token(), TelegramConnector

### Community 218 - "SKILL.md"
Cohesion: 0.18
Nodes (10): ADDED Requirements, Egress classes Specification Delta, Requirement: Capability packs MUST reference allowed egress classes, Requirement: The connector registry MUST type and protect egress endpoints, Requirement: The gate MUST enforce registry-rated egress classes, Scenario: A conflicting registration cannot downgrade an endpoint, Scenario: Pack egress classes become live grant authority, Scenario: Registered web endpoints expose stable classes (+2 more)

### Community 219 - "SKILL.md"
Cohesion: 0.18
Nodes (10): 1. Principal schema — authority-free, single-owner-shaped, 2. Identity store — DB-enforced single owner, audited binding, 3. IdentityResolver — read-only seam, owner fast path, 4. Composition cutover — principal_id, fail closed, 5. Owner-asserted binding — audited, owner-context-gated, agent-unreachable, Alternatives considered, Approach, Design: Implement identity store and principal (+2 more)

### Community 220 - "SKILL.md"
Cohesion: 0.18
Nodes (10): Alternatives rejected, Approach, Authority and containment, Connector broker resolution, Design: Secret intake and rotation, Failure modes, Intake state machine, Paired Gmail staging state machine (+2 more)

### Community 221 - "SKILL.md"
Cohesion: 0.18
Nodes (10): 1. Declaration schema — `openspine-schemas/src/nerve.rs`, 2. Kernel store — `openspine-kernel/src/store/nerve.rs`, 3. End-to-end admission, 4. Event-bus and storage discipline, 5. File layout, Alternatives considered, Approach, Authority sensitivity (+2 more)

### Community 222 - "SKILL.md"
Cohesion: 0.18
Nodes (11): Requirement: Action requests and gate decisions MUST be typed, Requirement: Commissioned workers MUST receive a briefcase, not the board (D-085), Requirement: Identity schemas MUST NOT grant runtime authority, Requirement: Route resolution schemas MUST represent ambiguity, Requirement: Route schemas MUST be declarative artifacts, Requirements, Scenario: Agent requests email thread read, Scenario: Briefcase scoped to the worker (`commissioning_persists_briefcase_without_board_row`) (+3 more)

### Community 224 - "lib.rs"
Cohesion: 0.20
Nodes (9): Check for context, Ending Discovery, Guardrails, OpenSpec Awareness, The Stance, What You Don't Have To Do, What You Might Do, When a change exists (+1 more)

### Community 242 - "lib.rs"
Cohesion: 0.20
Nodes (9): Authentication, Environment the shell process/container receives, Errors, `GET /v1/status`, `GET /v1/task`, OpenSpine kernel↔shell HTTP contract, `POST /v1/actions`, `POST /v1/model/generate` (+1 more)

### Community 245 - "mod.rs"
Cohesion: 0.20
Nodes (9): Check for context, Ending Discovery, Guardrails, OpenSpec Awareness, The Stance, What You Don't Have To Do, What You Might Do, When a change exists (+1 more)

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
Cohesion: 0.20
Nodes (9): Check for context, Ending Discovery, Guardrails, OpenSpec Awareness, The Stance, What You Don't Have To Do, What You Might Do, When a change exists (+1 more)

### Community 254 - "SKILL.md"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Define core runtime schemas, Summary, What Changes (+1 more)

### Community 268 - "content-assets.mjs"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Implement authority composition, Summary, What Changes (+1 more)

### Community 269 - "content-modules.mjs"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Implement digest-bound draft approval, Summary, What Changes (+1 more)

### Community 270 - "types.d.ts"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Implement gate action API, Summary, What Changes (+1 more)

### Community 274 - "SKILL.md"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Implement selected-thread email preview slice, Summary, What Changes (+1 more)

### Community 275 - "opsx-explore.md"
Cohesion: 0.20
Nodes (9): 1. Gmail connector skeleton, 2. Selection token, 3. Event and route, 4. Email read, 5. Model gateway, 6. Preview, 7. Tests, 8. Validation (+1 more)

### Community 276 - "opsx-apply.md"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Implement Telegram owner control slice, Summary, What Changes (+1 more)

### Community 277 - "opsx-archive.md"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Backfill implemented capability specs, Summary, What Changes (+1 more)

### Community 278 - "opsx-propose.md"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Harden approval and budgets, Summary, What Changes (+1 more)

### Community 279 - "SKILL.md"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Implement artifact lifecycle slice, Summary, What Changes (+1 more)

### Community 280 - "SKILL.md"
Cohesion: 0.20
Nodes (9): 1. Registry & schema plumbing, 2. Store, 3. Kernel: `artifact.propose`, 4. Kernel: approval branch + activation, 5. Fixtures + composition, 6. Shell: `/propose` UX, 7. Tests, 8. Validation (+1 more)

### Community 281 - "SKILL.md"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Refactor kernel registries, Summary, What Changes (+1 more)

### Community 282 - "TaskGrant"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Refactor pipeline driver, Summary, What Changes (+1 more)

### Community 283 - "ADDED Requirements"
Cohesion: 0.20
Nodes (9): ADDED Requirements, Requirement: EventEnvelope MUST carry an optional dormant thread_id, Requirement: TaskGrant MUST carry an optional dormant thread_id, Scenario: EventEnvelope with thread_id round-trips, Scenario: EventEnvelope without thread_id deserializes as None, Scenario: Mutating thread_id invalidates the grant MAC, Scenario: TaskGrant with thread_id round-trips, Scenario: TaskGrant without thread_id deserializes as None (+1 more)

### Community 284 - "Proposal: Refactor kernel registries"
Cohesion: 0.20
Nodes (9): Affected layer, Authority sensitivity, Decision-log check, Goals, Non-goals, Proposal: Implement identity store and principal, Summary, What Changes (+1 more)

### Community 286 - "Tasks: Refactor kernel registries"
Cohesion: 0.25
Nodes (9): stranded_worker_timeout_detects_expired(), handle_worker_commission(), commissioned_grant_for_receipt(), CommissionReceipt, mark_worker_stranded_notified(), record_worker_commissioned(), stranded_worker_timeouts(), worker_dispatch_state() (+1 more)

### Community 287 - "ApprovalRecord"
Cohesion: 0.20
Nodes (9): 1. Schemas, 2. Identity store, 3. IdentityResolver seam, 4. Composition cutover + bootstrap, 5. Owner-asserted binding path, 6. Tests, 7. Decision log + claims + docs, 8. Validation (+1 more)

### Community 288 - "fixtures.rs"
Cohesion: 0.20
Nodes (9): 1. Versioned Schema Migrations (`PRAGMA user_version`), 2. Boot Clock-Regression Detection, 3. Same-Conversation Serialization Guard, 4. Audit I/O Verification (Read-Only DB), 5. Backup & Restore Scope, Centralized Initialization & Safety Check, Design: Day-2 Operations Contract, Transactional Atomicity (+1 more)

### Community 289 - "D-052 — Archive applies deltas mechanically via `openspec archive --yes`; pre-seeded requirements are carried as MODIFIED; the `--skip-specs` hand-apply ceremony is retired"
Cohesion: 0.20
Nodes (9): ADDED Requirements, Requirement: Configurable caps, Requirement: Global daily spend ledger, Requirement: Lane-aware breach boundary, Scenario: Concurrent reservation cannot overspend, Scenario: Configured caps are enforced, Scenario: Counters persist by UTC day, Scenario: Immediate owner notification (+1 more)

### Community 290 - "D-030 — Telegram carries the entire owner-control UX for phases 1–3"
Cohesion: 0.20
Nodes (9): Briefcases to workers, not the board (D-085), Caveat-chain sub-grant minting (offline-verifiable), Design: Worker runtime commissioning and reply chokepoint, Master: interpret / commission / relay, Narrowing caveats (never widening), Receipt-keyed, fail-closed terminal flip (D-083), Reply chokepoint: no output channels on worker grants (AD-035), Tests (acceptance) (+1 more)

### Community 291 - "artifact_propose.rs"
Cohesion: 0.20
Nodes (10): Agent-OS change sequence (2026-07-07, AD canon), Event substrate, implement-counterparty-key-model, implement-durable-workflow-replay, implement-event-bus-subscriptions, implement-overlay-export-restore, implement-overlay-model, implement-task-board (+2 more)

### Community 292 - "Overlay & key model"
Cohesion: 0.31
Nodes (4): PromotionDenial, record_verdict(), run_promotion_review(), SkillReviewPassed

### Community 293 - "Proposal: Define grant chain and modes"
Cohesion: 0.25
Nodes (7): ActivationCommit, ArtifactRef, Option, Result, StandingRuleManifest, Ulid, Store

### Community 294 - "D-010 — Model calls with private context go through model gateway"
Cohesion: 0.22
Nodes (8): 1. Create schema location, 2. Define event schemas, 3. Define identity and route schemas, 4. Define authority schemas, 5. Define action/approval/model/audit schemas, 6. Verification, 7. Review, Tasks: Define core runtime schemas

### Community 295 - "D-019 — Implement minimal slice first, not full agent OS"
Cohesion: 0.22
Nodes (8): 1. Add development-process spec, 2. Add development-process design, 3. Add proposal, 4. Strengthen OpenSpec config, 5. Review consistency with existing PRD and decision log, 6. Prepare future changes, 7. Archive readiness, Tasks: Define OpenSpine development process

### Community 296 - "Q: Trace the full runtime call path for a single incoming Telegram message from an authorized owner, all the way through to a TaskGrant being persisted in the kernel's SQLite store. List the modules/files involved, in call order, and identify the 2-3 modules that are most structurally central to this flow."
Cohesion: 0.22
Nodes (8): 1. Immutable draft artifact, 2. Approval record, 3. Gate integration, 4. Gmail draft action, 5. Audit, 6. Tests, 7. Validation, Tasks: Implement digest-bound draft approval

### Community 297 - "Blindspot pass"
Cohesion: 0.22
Nodes (8): 1. Telegram connector, 2. Event normalization, 3. Routing and authority, 4. Actions, 5. Tests, 6. Documentation, 7. Validation, Tasks: Implement Telegram owner control slice

### Community 298 - "Delegation & containment"
Cohesion: 0.22
Nodes (8): 1. ActionCatalog (schemas) + fail-fast wiring (authority, gate), 2. ActionHandlerRegistry (kernel/api), 3. ConnectorRegistry (kernel), 4. Artifact-kind table (kernel), Alternatives considered, Approach, Design: Refactor kernel registries, Key decisions

### Community 299 - "SelectionToken"
Cohesion: 0.22
Nodes (8): 1. Typed stage sequence — consumed, not decorative, 2. Lanes as data — with a hard hook boundary, 3. Cutover — code moves, it does not accrete, 4. What does NOT move, Alternatives considered, Approach, Design: Refactor pipeline driver, Key decisions (D-054)

### Community 300 - "tasks.md"
Cohesion: 0.22
Nodes (8): ADDED Requirements, MODIFIED Requirements, Requirement: Audit schemas MUST reference private payloads by encrypted or hash refs, Requirement: Event bus subscription types MUST be explicit schemas, Scenario: Audit event carries aggregate stream coordinates, Scenario: Filter and checkpoint types exist, Scenario: Model request includes private email content, Spec: Core runtime schemas (event bus extensions)

### Community 301 - "Implementation notes"
Cohesion: 0.22
Nodes (8): Acceptance Criteria, Dependencies, Non-Goals, Problem, Proposal: Implement Day-2 Operations Contract, Proposed Solution, Ratified decisions, Scope

### Community 302 - "tests.rs"
Cohesion: 0.22
Nodes (8): AD-110 promotion review (mined only), Candidate decision-log entry (UNNUMBERED — formal revisit of D-048), Deferred / candidates (unnumbered), Design: Skill artifact class, Provenance-gated ceremony (AD-041), Read-only matcher (AD-042), Test mapping, Type-level structural containment (AD-040)

### Community 303 - "get_task"
Cohesion: 0.43
Nodes (6): WorkerBoundParameterPayload, WorkerCommissionPayload, WorkerCommissionResponse, WorkerReportPayload, WorkerRequestPayload, WorkerSlotPayload

### Community 304 - "Approach"
Cohesion: 0.33
Nodes (7): grant(), GrantLimits, GrantMode, legacy_missing_chain_defaults_but_fails_closed(), legacy_without_thread_id_defaults_to_none(), round_trip_and_mac(), thread_id_round_trips_when_populated()

### Community 305 - "Approach"
Cohesion: 0.39
Nodes (7): handoff_complete(), run(), kernel_notify_grant(), notify_owner_required_outcome(), notify_owner_with_digest(), NotifyOutcome, record_notify_skipped()

### Community 306 - "action_catalog.rs"
Cohesion: 0.29
Nodes (5): ActionHandler, ActionHandlerRegistry, HashMap, Self, Vec

### Community 307 - ".fmt"
Cohesion: 0.39
Nodes (6): consult_standing_rule_gate(), Option, Result, Store, String, StandingRuleGateOutcome

### Community 308 - "artifact_propose.rs"
Cohesion: 0.43
Nodes (7): PostApprovalFuture, handle_activate_artifact(), handle_create_approved_draft(), handle_nominate_upstream(), handle_reconfirm_artifact(), handle_resolve_approved_plan(), handle_unknown_approved_action()

### Community 309 - "effect_paths.rs"
Cohesion: 0.25
Nodes (8): Change Log, Consequences, D-107 — Standing rules concretize AD-012 dark-window defaults, Decision, Open Decision Questions — CLOSED (see linked decisions), Rationale, Research / Reference Backlog, Would change if

### Community 310 - "Overlay & key model"
Cohesion: 0.25
Nodes (7): Context, Design: Authority composition, Inputs, Merge rule, Output, Precedence, Tests

### Community 311 - "retry_worker.rs"
Cohesion: 0.25
Nodes (7): 1. New capability specs, 2. Restore dropped dev-process requirements, 3. Close the loophole going forward, 4. Docs, 5. Decision log, 6. Validation, Tasks: Backfill implemented capability specs

### Community 312 - "D-014 — Bootstrap/setup secrets bypass shell/model context"
Cohesion: 0.25
Nodes (7): ADDED Requirements, Requirement: Approval MUST bind only what the owner was shown, Requirement: Approvals MUST expire, Scenario: Approval has expired, Scenario: Preview fits without truncation, Scenario: Preview must be truncated, Spec: Digest-bound draft approval

### Community 313 - "editUrl"
Cohesion: 0.25
Nodes (7): ADDED Requirements, Requirement: Grant limits MUST be enforced at runtime, Requirement: Kernel-originated owner notifications are a trusted, audited path, Scenario: Kernel sends a courtesy notice, Scenario: Model call beyond the budget, Scenario: Shell-initiated artifact creation beyond the budget, Spec: Gate action API

### Community 314 - "Proposal: Implement identity store and principal"
Cohesion: 0.25
Nodes (7): 1. ActionCatalog + fail-fast, 2. ConnectorRegistry, 3. ActionHandlerRegistry, 4. Artifact-kind table, 5. Decision log + docs, 6. Validation, Tasks: Refactor kernel registries

### Community 315 - "Tasks: Implement identity store and principal"
Cohesion: 0.25
Nodes (7): 1. Typed stage sequence, 2. LaneSpec + lane constructors, 3. Driver + cutover, 4. Tests, 5. Decision log + docs, 6. Validation, Tasks: Refactor pipeline driver

### Community 316 - "Option"
Cohesion: 0.25
Nodes (7): Alternatives considered, Authority posture, Design: define-lineage-and-eval-store, Eval-verdict store, Lineage model, Migration strategy, Risks

### Community 317 - "Ulid"
Cohesion: 0.25
Nodes (7): Acceptance Criteria, Dependencies, Out of Scope, Problem/Context, Proposal: Implement escalation and refusal, Proposed Solution, Summary

### Community 318 - "Vec"
Cohesion: 0.25
Nodes (7): Acceptance Criteria, Context, Dependencies, Out of Scope, Problem, Proposal: Model swap ceremony, Proposed Solution

### Community 319 - "HashMap"
Cohesion: 0.25
Nodes (7): MODIFIED Requirements, Requirement: ActionCatalog MUST enumerate every trusted-path carve-out around gate(), Scenario: Each enumerated entry has a dedicated test, Scenario: Model activation is classified, Scenario: Model golden-set execution is classified, Scenario: The carve-out set is finite and enumerated, Spec: Gate action API

### Community 320 - "benchmark.rs"
Cohesion: 0.25
Nodes (7): Acceptance Criteria, Context, Dependencies, Out of Scope, Problem, Proposal: Overlay evaluation gate, Proposed Solution

### Community 321 - "Timestamp"
Cohesion: 0.25
Nodes (7): Acceptance Criteria, Dependencies, Impact, Out of Scope, Problem/Context, Proposal: Plan digest-bound approval, Proposed Solution

### Community 322 - "head"
Cohesion: 0.25
Nodes (7): Acceptance Criteria, Dependencies, Out of Scope, Problem, Proposal: Implement durable workflow replay, Proposed Solution, Ratified decisions

### Community 323 - "Vec"
Cohesion: 0.25
Nodes (7): 1. OpenSpec and authority, 2. Encrypted vault, 3. Gate-mediated intake mode, 4. Connector wiring, 5. Acceptance tests, 6. Verification, Tasks: Implement secret intake

### Community 324 - "lib.rs"
Cohesion: 0.25
Nodes (8): Authority-sensitive changes, Change structure, Naming, OpenSpec boundary, OpenSpine conventions, Purpose, Requirement language, Verification

### Community 325 - "Arc"
Cohesion: 0.25
Nodes (4): Canon sources, Completed / archived, OpenSpine OpenSpec change sequence, Reconciliation of the previous "later changes" list

### Community 326 - "Error"
Cohesion: 0.25
Nodes (8): Docs, Every claim has a test, License, OpenSpine, Status, Try it in 5 minutes, What is this?, Why it's different

### Community 327 - "D-015 — Phase 1 should avoid final email send"
Cohesion: 0.25
Nodes (5): reinstate_artifact(), OwnerReconfirmation, Store, Provenance, ReconfirmAnchor

### Community 328 - "D-016 — Capability packs are candidate profiles, not live authority"
Cohesion: 0.48
Nodes (6): canonical_catalog(), counterparty_classification_is_kernel_owned_and_fails_closed(), handler_registry_requires_explicit_classification(), id(), test_catalog_effect_paths_are_fully_enumerated_and_classified(), worker_actions_declare_no_egress_and_no_output_channel()

### Community 329 - "Option"
Cohesion: 0.29
Nodes (6): dispatch_read_selected_thread(), ReadThreadPayload, Option, Result, String, Value

### Community 330 - "Result"
Cohesion: 0.29
Nodes (6): 1. Create the Telegram bot, 2. Find your Telegram user ID — owner identity, verified structurally, 3. Generate the artifact encryption key, 4. Minimal `openspine.yaml`, 5. Unsafe dev shortcuts (do not carry into production), Telegram owner-control setup (Phase 1)

### Community 331 - "State"
Cohesion: 0.29
Nodes (6): 1. Composer interface, 2. Merge logic, 3. Tests, 4. Documentation, 5. Validation, Tasks: Implement authority composition

### Community 332 - "StatusCode"
Cohesion: 0.29
Nodes (6): Approval record, Design: Digest-bound draft approval, Final send, Flow, Gate behavior, Immutable artifact

### Community 333 - "String"
Cohesion: 0.29
Nodes (6): Audit, Behavior, Connector execution, Design: Gate action API, Gate responsibility, Interface

### Community 334 - "Ulid"
Cohesion: 0.29
Nodes (6): 1. Types, 2. Gate implementation, 3. Audit, 4. Tests, 5. Validation, Tasks: Implement gate action API

### Community 335 - "Value"
Cohesion: 0.29
Nodes (6): Allowed actions, Design: Selected-thread email preview slice, Email content trust, Flow, Output, Selection token

### Community 336 - "MockServer"
Cohesion: 0.29
Nodes (6): Design: Telegram owner control slice, Flow, Main assistant authority, Polling vs webhook, Secret intake, Verification

### Community 337 - "TelegramConnector"
Cohesion: 0.29
Nodes (6): ADDED Requirements, Requirement: Selection tokens MUST be single-use, expiring, and scope-fixed, Scenario: Token reused after consumption, Scenario: Token used after expiry, Scenario: Token used by a foreign grant, Spec: Selected-thread email preview slice

### Community 338 - "D-032 — Kernel↔shell transport is HTTP/JSON with a per-task bearer token"
Cohesion: 0.29
Nodes (6): Chained HMAC construction, Design: Define grant chain and modes, Gate decisions and shadow, Immutable root authority + append-only caveats, Macaroons-simple chain (AD-101), Out of scope

### Community 340 - "D-033 — Action identifiers are exact-match dotted strings; unverified senders are audited and ignored"
Cohesion: 0.29
Nodes (6): MODIFIED Requirements, Requirement: Task grants MUST be explicit live authority objects, Scenario: Bound parameters are caveats, Scenario: Root grant defaults, Scenario: Sub-grant is the sole presented authority, Spec: Core runtime schemas

### Community 341 - "MockServer"
Cohesion: 0.29
Nodes (6): Acceptance Criteria, Change: define-lineage-and-eval-store, Dependencies, Out of Scope, Problem/Context, Proposed Solution

### Community 342 - "Timestamp"
Cohesion: 0.29
Nodes (6): Alternatives rejected, Authority boundary, Compatibility, Design: Egress classes, Grant and MAC model, Registry ratings

### Community 343 - "Value"
Cohesion: 0.29
Nodes (6): Acceptance Criteria, Dependencies, Out of Scope, Problem/Context, Proposal: Implement egress classes, Proposed Solution

### Community 344 - "Arc"
Cohesion: 0.43
Nodes (5): WorkerBoundParameter, WorkerOutcome, WorkerRequest, WorkerResult, WorkerSlot

### Community 345 - "HeaderMap"
Cohesion: 0.29
Nodes (6): 1. OpenSpec artifacts, 2. Schema types (openspine-schemas), 3. Construction site updates, 4. Kernel integration (openspine-kernel), 5. Local gate, Tasks: implement-escalation-and-refusal

### Community 346 - "D-030 — Telegram carries the entire owner-control UX for phases 1–3"
Cohesion: 0.29
Nodes (6): Acceptance Criteria, Dependencies, Out of Scope, Problem/Context, Proposal: Implement event bus subscriptions, Proposed Solution

### Community 347 - "Option"
Cohesion: 0.29
Nodes (6): ADDED Requirements, Requirement: Audit append MUST assign per-aggregate sequence under the store lock, Requirement: The store MUST support filtered ordered replay of the audit ledger, Scenario: Replay after watermark skips earlier rows, Scenario: Sequential appends on one aggregate, Spec: Audit artifact store (event bus ledger extensions)

### Community 348 - "Result"
Cohesion: 0.29
Nodes (6): 1. Schemas, 2. Ledger write path, 3. Replay + idempotent consumer, 4. Tests, 5. Validation, Tasks: Implement event bus subscriptions

### Community 351 - "String"
Cohesion: 0.29
Nodes (6): MODIFIED Requirements, Requirement: Identity schemas MUST NOT grant runtime authority, Requirement: OpenSpine core runtime objects MUST have explicit schemas, Scenario: Known owner identity exists, Scenario: Runtime object is added, Spec: Core runtime schemas

### Community 354 - "JoinHandle"
Cohesion: 0.29
Nodes (6): Authority boundary, Canonical plan payload, Design: Plan digest-bound approval, Mutation refusal, One-loop question and kernel response, Verification strategy

### Community 355 - "Option"
Cohesion: 0.29
Nodes (6): Acceptance Criteria, Dependencies, Out of Scope, Problem, Proposal: Kernel briefcase packing, Proposed Solution

### Community 356 - "Response"
Cohesion: 0.29
Nodes (6): Design: Durable workflow replay, Exact step identity, Gated, approval, and private boundaries, Ledger aggregate and verified replay, Ratified decisions, Timer substrate

### Community 357 - "SocketAddr"
Cohesion: 0.29
Nodes (6): Acceptance Criteria, Dependencies, Failure surfacing contract, Out of Scope, Problem/Context, Proposed Solution

### Community 358 - "HashMap"
Cohesion: 0.29
Nodes (6): Acceptance Criteria, Dependencies, Out of Scope, Problem/Context, Proposal: Implement overlay artifact compatibility, Proposed Solution

### Community 360 - "Self"
Cohesion: 0.29
Nodes (6): Declarative format (D-087..D-090), Design: minimal seed workflow set, Out of scope, Overlay shipping, not kernel fixtures (AD-070/AD-071/AD-080), Security, Seed selection and ids (AD-153)

### Community 361 - "Value"
Cohesion: 0.29
Nodes (11): ArtifactLoadError, load_admitted_personas(), load_registry_into(), load_registry_without_personas(), load_yaml_dir(), F, Path, PathBuf (+3 more)

### Community 362 - "Arc"
Cohesion: 0.80
Nodes (4): advisee_scope(), concurrent_cross_connection_admission_spends_once(), declaration(), provenance()

### Community 363 - "Display"
Cohesion: 0.29
Nodes (6): Bounded master read-model, Design: Kernel task board, Failure and replay behavior, Task objects and persistence, Timer dispatch invariants, Timers and normal authority path

### Community 364 - "HeaderMap"
Cohesion: 0.29
Nodes (6): Acceptance Criteria, Dependencies, Out of Scope, Problem/Context, Proposal: Implement nerve subscribers, Proposed Solution

### Community 365 - "Json"
Cohesion: 0.29
Nodes (6): 1. Declaration schema (schemas crate), 2. Kernel store module, 3. Interjection admission, 4. Tests, 5. Verification, Tasks: Implement nerve subscribers

### Community 366 - "Option"
Cohesion: 0.23
Nodes (11): ArtifactRef, Connection, Digest, Option, Result, Ulid, Vec, Store (+3 more)

### Community 367 - "Result"
Cohesion: 0.29
Nodes (6): Acceptance Criteria, Dependencies, Out of Scope, Problem/Context, Proposal: implement-personality-seed, Proposed Solution

### Community 368 - "State"
Cohesion: 0.29
Nodes (6): Acceptance Criteria, Dependencies, Out of Scope, Problem, Proposal: Worker runtime commissioning and reply chokepoint, Proposed Solution

### Community 369 - "StatusCode"
Cohesion: 0.29
Nodes (7): Authority growth, implement-disclosure-policy, implement-egress-classes, implement-model-swap-ceremony, implement-overlay-eval-gate, implement-plan-digest-approval, implement-standing-rules

### Community 370 - "Ulid"
Cohesion: 0.29
Nodes (7): define-grant-chain-and-modes, define-lineage-and-eval-store, harden-gate-trusted-paths, implement-identity-store-and-principal, Kernel foundation, refactor-kernel-registries, refactor-pipeline-driver

### Community 371 - "Value"
Cohesion: 0.29
Nodes (7): Requirement: Authority-bearing proposals require overlay evaluation before approval, Scenario: Generic lifecycle bypass is rejected, Scenario: Missing model-swap evaluation blocks approval, Scenario: Model swap lifecycle bypass is rejected, Scenario: Model swap with two passing evaluations reaches approval, Scenario: Proposal with two passing evaluations reaches approval, Scenario: Proposal without captured owner history is denied

### Community 372 - "Vec"
Cohesion: 0.33
Nodes (5): Artifact and ceremony, Atomic budget consultation, Dark windows, Design, Module boundaries

### Community 373 - "Arc"
Cohesion: 0.70
Nodes (4): activate_overlay_yaml(), prune_non_highest_active(), remove_loaded_version(), republish_missing_committed()

### Community 378 - "HeaderMap"
Cohesion: 0.33
Nodes (6): Agentic decisions, D-008 — Deterministic routing decides authority; agentic routing decides strategy, Decision, Deterministic decisions, Rationale, Would change if

### Community 379 - "Json"
Cohesion: 0.33
Nodes (6): Consequences, D-001 — Lyra is a runtime/substrate, not a single agent, Decision, Rationale, Trade-offs, Would change if

### Community 380 - "Option"
Cohesion: 0.33
Nodes (6): Consequences, D-039 — Draft-approval channel is a Telegram inline button (`callback_query`), not a text command, Decision, Rationale, Trade-offs, Would change if

### Community 381 - "Result"
Cohesion: 0.33
Nodes (6): Consequences, D-042 — Reply recipient is kernel-derived, never shell-supplied: newest non-owner sender, matched against a configured mailbox address, Decision, Rationale, Trade-offs, Would change if

### Community 382 - "State"
Cohesion: 0.33
Nodes (6): Consequences, D-002 — First usable UX should include an owner control channel, Decision, Rationale, Trade-offs, Would change if

### Community 383 - "StatusCode"
Cohesion: 0.33
Nodes (6): Consequences, D-003 — Gmail is a guarded workflow, not the whole product, Decision, Rationale, Trade-offs, Would change if

### Community 385 - "Ulid"
Cohesion: 0.33
Nodes (6): Consequences, D-004 — Every effectful action goes through `gate()`, Decision, Effectful actions include, Rationale, Would change if

### Community 386 - "Value"
Cohesion: 0.33
Nodes (6): Consequences, D-005 — Private-data shell must be contained, Decision, Rationale, Required containment, Would change if

### Community 387 - "JoinHandle"
Cohesion: 0.33
Nodes (6): Consequences, D-009 — External content is data, not instruction, Decision, Examples, Rationale, Would change if

### Community 388 - "Option"
Cohesion: 0.33
Nodes (6): Consequences, D-021 — Email domain is broader than Gmail, Decision, Rationale, Trade-offs, Would change if

### Community 389 - "Response"
Cohesion: 0.33
Nodes (6): Consequences, D-022 — Agent-owned inbox is distinct from owner mailbox access, Decision, Distinction, Rationale, Would change if

### Community 390 - "SocketAddr"
Cohesion: 0.33
Nodes (6): Consequences, D-023 — OpenSpine is the substrate; Lyra is a product built on it, Decision, Positioning, Rationale, Would change if

### Community 391 - "HashMap"
Cohesion: 0.33
Nodes (6): Consequences, D-024 — OpenSpec is the development/change-management layer, not the runtime, Decision, Mapping, Rationale, Would change if

### Community 392 - "Option"
Cohesion: 0.33
Nodes (6): Consequences, D-026 — Shell containment via a `SandboxDriver` trait (Process dev-only / Docker), Decision, Rationale, Trade-offs, Would change if

### Community 393 - "Self"
Cohesion: 0.33
Nodes (5): Budget enforcement placement (D-046), Design: Harden approval and budgets, Task-token hashing and sweep (D-047), Trusted notification audit (D-046 continued), WYSIWYS (D-045)

### Community 394 - "Value"
Cohesion: 0.33
Nodes (5): Acceptance Criteria, Non-goals, Proposal: Define grant chain and modes, Summary, What Changes

### Community 395 - "Arc"
Cohesion: 0.33
Nodes (5): ADDED Requirements, Requirement: Authority-bearing proposals require overlay evaluation before approval, Scenario: Generic lifecycle bypass is rejected, Scenario: Proposal with two passing evaluations reaches approval, Scenario: Proposal without captured owner history is denied

### Community 396 - "Display"
Cohesion: 0.33
Nodes (5): 1. Author the four seed WorkflowManifests, 2. Embed and materialize seeds as overlay artifacts, 3. Verify the acceptance criteria, 4. Local gate, Tasks: implement-seed-workflows

### Community 397 - "HeaderMap"
Cohesion: 0.33
Nodes (5): Boundary, Configuration, Design, Ledger, Model and connector accounting

### Community 398 - "Json"
Cohesion: 0.33
Nodes (5): Design: Workflow state machines, Gateway routing, Manifest shape, Runtime, Security and replay

### Community 399 - "Option"
Cohesion: 0.33
Nodes (5): Anti-pattern probes (AD-081 / AD-083), Design: implement-personality-seed, Overlay machinery reuse (D-077..D-081), Seed content (AD-080 eight + AD-082 default), What this change deliberately does NOT do

### Community 400 - "Result"
Cohesion: 0.33
Nodes (6): Failure surfacing & operations, implement-connector-reality, implement-day2-operations, implement-failure-surfacing-contract, implement-secret-intake, implement-spend-kill-switch

### Community 401 - "State"
Cohesion: 0.53
Nodes (4): dispatch_skill_context(), skill_context_rejects_unknown_grant_purpose(), skill_context_selects_only_grant_scoped_installed_matches(), task_class_from_grant_purpose()

### Community 403 - "Ulid"
Cohesion: 0.40
Nodes (4): Change: Implement standing rules, Impact, What Changes, Why

### Community 404 - "Value"
Cohesion: 0.40
Nodes (4): 1. Schema and persistence, 2. Gate-time enforcement, 3. Durable dark windows, 4. Ceremony and verification

### Community 405 - "D-016 — Capability packs are candidate profiles, not live authority"
Cohesion: 0.40
Nodes (5): Consequences, D-035 — Kernel advertises a separate `advertise_endpoint` from its `bind_addr`; no Unix-domain-socket transport for `ProcessDriver`, Decision, Rationale, Would change if

### Community 406 - "D-017 — Personas grant no authority"
Cohesion: 0.40
Nodes (5): Consequences, D-036 — Phase-2 thread selection is a kernel-recognized `/draft <thread_id>` command, not free-form NLU or a shell-supplied id, Decision, Rationale, Would change if

### Community 407 - "D-018 — Routes are declarative artifacts, not kernel code"
Cohesion: 0.40
Nodes (5): Consequences, D-037 — Gmail OAuth via a plain refresh-token POST (no `oauth2` crate); `base64` promoted from transitive to direct dependency, Decision, Rationale, Would change if

### Community 408 - "Json"
Cohesion: 0.40
Nodes (5): Consequences, D-038 — `resolve_owner_identity`'s `channel_trust` is caller-supplied, not hardcoded, Decision, Rationale, Would change if

### Community 409 - "Result"
Cohesion: 0.40
Nodes (5): Consequences, D-040 — Pending (pre-approval) `ActionRequest`s are persisted in a new `action_requests` table, Decision, Rationale, Would change if

### Community 418 - "Response"
Cohesion: 0.40
Nodes (5): Consequences, D-041 — `email.create_draft`'s digest composition: payload = `{subject, body}`, target = `{thread_id, connector, account_role, recipients}`, Decision, Rationale, Would change if

### Community 419 - "D-027 — Multi-provider model gateway with per-provider auth mode"
Cohesion: 0.40
Nodes (5): Consequences, D-043 — `lyra.ui.preview` is extended (not duplicated) to propose the exact reviewed draft and attach the approval button, Decision, Rationale, Would change if

### Community 420 - "D-028 — Canonical artifact format is YAML+serde; digests are `sha256:` over canonical JSON"
Cohesion: 0.40
Nodes (5): Consequences, D-044 — Approved draft creation dispatches kernel-side; no new shell spawn, Decision, Rationale, Would change if

### Community 421 - "ArtifactId"
Cohesion: 0.40
Nodes (5): Consequences, D-045 — WYSIWYS: a truncated preview refuses an approval button rather than splitting the message, Decision, Rationale, Would change if

### Community 422 - "CapabilityPack"
Cohesion: 0.40
Nodes (5): Consequences, D-046 — Grant budgets are enforced kernel-dispatch-side; the artifact budget counts only shell-initiated puts, Decision, Rationale, Would change if

### Community 424 - "F"
Cohesion: 0.40
Nodes (5): Consequences, D-047 — Task tokens are hashed at rest; expired grants are swept, Decision, Rationale, Would change if

### Community 425 - "HashMap"
Cohesion: 0.40
Nodes (5): Consequences, D-048 — `artifact.activate` is the single canonical activation action id; every runtime proposal requires uniform owner approval; prompt templates are excluded from proposable kinds, Decision, Rationale, Would change if

### Community 426 - "Option"
Cohesion: 0.40
Nodes (5): Consequences, D-049 — Capability specs are backfilled for subsystems implemented inside earlier slices, Decision, Rationale, Would change if

### Community 427 - "Path"
Cohesion: 0.40
Nodes (5): Consequences, D-050 — `max_model_calls` is enforced with an atomic upsert, not a count-then-compare, Decision, Rationale, Would change if

### Community 433 - "WorkflowManifest"
Cohesion: 0.40
Nodes (5): Consequences, D-051 — The agent-OS canon (AD-001..153) is decomposed into a dependency-edged change sequence; the stale later-changes placeholders are superseded or subsumed, Decision, Rationale, Would change if

### Community 436 - "PathBuf"
Cohesion: 0.40
Nodes (5): Consequences, D-052 — Archive applies deltas mechanically via `openspec archive --yes`; pre-seeded requirements are carried as MODIFIED; the `--skip-specs` hand-apply ceremony is retired, Decision, Rationale, Would change if

### Community 438 - "Self"
Cohesion: 0.40
Nodes (5): Consequences, D-053 — Kernel extension points are compiled-in registries; a curated canonical `ActionCatalog` makes unknown action ids fail fast at composition and gate, Decision, Rationale, Would change if

### Community 439 - "String"
Cohesion: 0.40
Nodes (5): Consequences, D-054 — Pipeline stages are a typed compiled-in sequence the driver executes; lanes are compiled-in data records, Decision, Rationale, Would change if

### Community 440 - "Vec"
Cohesion: 0.40
Nodes (5): Consequences, D-055 — Gate trusted paths are hardened: carve-outs are enumerated catalog data; KernelOrigin is approval-exempt, audit-never-exempt; selection-token validation lives in pure gate() with dispatch-side consumption; digests are kernel-re-derived at approval-effect time, Decision, Rationale, Would change if

### Community 441 - "Result"
Cohesion: 0.40
Nodes (5): Consequences, D-056 — Eval-store groundwork defers AD-111 evaluator policy: only the verdict-landing surface is settled, Decision, Rationale, Would change if

### Community 442 - "Error"
Cohesion: 0.40
Nodes (5): Consequences, D-057 — Counterparty-facing actions are an explicit kernel catalog set, Decision, Rationale, Would change if

### Community 445 - "PathBuf"
Cohesion: 0.40
Nodes (5): Consequences, D-058 — Security escalations require result-returning owner delivery, Decision, Rationale, Would change if

### Community 446 - "Result"
Cohesion: 0.40
Nodes (5): Consequences, D-059 — Dormant thread bindings are MAC-authenticated before activation, Decision, Rationale, Would change if

### Community 447 - "Self"
Cohesion: 0.40
Nodes (5): Consequences, D-060 — The overlay eval gate's first-cut evaluator is deterministic; the full replay/judge protocol is owner-reserved, Decision, Rationale, Would change if

### Community 448 - "String"
Cohesion: 0.40
Nodes (5): Consequences, D-061 — Model-swap golden sets use a bounded deterministic first cut, Decision, Rationale, Would change if

### Community 449 - "Vec"
Cohesion: 0.40
Nodes (5): Consequences, D-062 — Active model swaps require symmetric DB and overlay provenance, Decision, Rationale, Would change if

### Community 450 - "Client"
Cohesion: 0.40
Nodes (5): Consequences, D-063 — Model-swap activation uses a serialized staged recovery protocol, Decision, Rationale, Would change if

### Community 451 - "Error"
Cohesion: 0.40
Nodes (5): Consequences, D-064 — Connector secrets migrate once into the kernel vault, Decision, Rationale, Would change if

### Community 452 - "Mutex"
Cohesion: 0.40
Nodes (5): Consequences, D-065 — Provider API-key vault migration belongs to foundation amendment, Decision, Rationale, Would change if

### Community 453 - "Option"
Cohesion: 0.40
Nodes (5): Consequences, D-066 — Paired Gmail credentials stage until atomic validated promotion, Decision, Rationale, Would change if

### Community 454 - "Result"
Cohesion: 0.40
Nodes (5): Consequences, D-067 — Telegram poll offsets are namespaced by bot identity, Decision, Rationale, Would change if

### Community 455 - "Self"
Cohesion: 0.40
Nodes (5): Consequences, D-068 — Authenticated API bad requests are not duplicated through owner notification, Decision, Rationale, Would change if

### Community 456 - "String"
Cohesion: 0.40
Nodes (5): Consequences, D-069 — Kernel connector counters are the minimal observability surface, Decision, Rationale, Would change if

### Community 457 - "Timestamp"
Cohesion: 0.40
Nodes (5): Consequences, D-070 — Retryable owner notifications use encrypted artifact references, Decision, Rationale, Would change if

### Community 458 - "Value"
Cohesion: 0.40
Nodes (5): Consequences, D-071 — External owner delivery may be delivery-unknown after a crash, Decision, Rationale, Would change if

### Community 459 - "Vec"
Cohesion: 0.40
Nodes (5): Consequences, D-072 — Digest detail retrieval is a secure lossless pagination substrate, Decision, Rationale, Would change if

### Community 460 - "GmailConnector"
Cohesion: 0.40
Nodes (5): Consequences, D-073 — Durable workflow steps persist intent before effect and replay recorded outcomes, Decision, Rationale, Would change if

### Community 461 - "template"
Cohesion: 0.40
Nodes (5): Consequences, D-074 — Workflow timers fire at most once via trusted-clock atomic claims, Decision, Rationale, Would change if

### Community 462 - "Value"
Cohesion: 0.40
Nodes (5): Consequences, D-075 — The spend kill switch accounts globally but pauses only non-immediate lanes, Decision, Rationale, Would change if

### Community 463 - "Option"
Cohesion: 0.40
Nodes (5): Consequences, D-076 — Spend caps are required finite configuration, Decision, Rationale, Would change if

### Community 465 - "Self"
Cohesion: 0.40
Nodes (5): Consequences, D-077 — Learned artifacts carry exchange provenance and reconfirmations record a durable anchor, Decision, Rationale, Would change if

### Community 469 - "Option"
Cohesion: 0.40
Nodes (5): Consequences, D-078 — Owner reconfirmation is digest-bound with a durable owner-accepted disposition, Decision, Rationale, Would change if

### Community 470 - "String"
Cohesion: 0.40
Nodes (5): Consequences, D-080 — Legacy migration is discovery and quarantine only, Decision, Rationale, Would change if

### Community 471 - "Vec"
Cohesion: 0.40
Nodes (5): Consequences, D-081 — Upstream nomination is explicit depersonalized opt-in, Decision, Rationale, Would change if

### Community 472 - "Client"
Cohesion: 0.40
Nodes (5): Consequences, D-082 — Task-board timer consumption is transactionally idempotent, Decision, Rationale, Would change if

### Community 473 - "Error"
Cohesion: 0.40
Nodes (5): Consequences, D-083 — Task dispatch is atomic with receipt-keyed fail-closed recovery, Decision, Rationale, Would change if

### Community 474 - "Result"
Cohesion: 0.40
Nodes (5): Consequences, D-084 — Task slices are deterministic bounded projections, Decision, Rationale, Would change if

### Community 475 - "Self"
Cohesion: 0.40
Nodes (5): Consequences, D-085 — Briefcase task classes derive deterministically from the dispatch lane, Decision, Rationale, Would change if

### Community 476 - "String"
Cohesion: 0.40
Nodes (5): Consequences, D-086 — Selected-thread email preflight is a bounded pre-gate metadata snapshot, Decision, Rationale, Would change if

### Community 477 - "Value"
Cohesion: 0.40
Nodes (5): Consequences, D-088 — A workflow transition writes exactly one advancing durable step, Decision, Rationale, Would change if

### Community 479 - "Result"
Cohesion: 0.40
Nodes (5): Consequences, D-089 — Reasoning-tier routing resolves the active provider per call, Decision, Rationale, Would change if

### Community 480 - "Option"
Cohesion: 0.40
Nodes (5): Consequences, D-090 — Workflow manifests are digest-bound at run start; production driving is deferred, Decision, Rationale, Would change if

### Community 481 - "Result"
Cohesion: 0.40
Nodes (5): Consequences, D-091 — Seed workflows ship as overlay artifacts through the standard path, Decision, Rationale, Would change if

### Community 482 - "String"
Cohesion: 0.40
Nodes (5): Consequences, D-092 — Nerve admission and replay are kernel-owned boundaries, Decision, Rationale, Would change if

### Community 483 - "Timestamp"
Cohesion: 0.40
Nodes (5): Consequences, D-093 — Seeded nerves default to the Cheap model tier, Decision, Rationale, Would change if

### Community 484 - "Vec"
Cohesion: 0.40
Nodes (5): Consequences, D-094 — Persona is a seventh overlay artifact kind with no authority, Decision, Rationale, Would change if

### Community 485 - "Box"
Cohesion: 0.40
Nodes (5): Consequences, D-095 — Kernel-authored bootstrap provenance for seeded personas, Decision, Rationale, Would change if

### Community 486 - "Result"
Cohesion: 0.40
Nodes (5): Consequences, D-096 — Deterministic personality probes; digest format is a learnable default, Decision, Rationale, Would change if

### Community 489 - "Timestamp"
Cohesion: 0.40
Nodes (5): Consequences, D-097 — Persona overlay loading is admission-gated, Decision, Rationale, Would change if

### Community 490 - "Ulid"
Cohesion: 0.40
Nodes (5): Consequences, D-098 — Gmail draft writes keep durable pending evidence, Decision, Rationale, Would change if

### Community 491 - "Value"
Cohesion: 0.40
Nodes (5): Consequences, D-099 — Connector breakers use sliding-window failure accounting, Decision, Rationale, Would change if

### Community 492 - "MockServer"
Cohesion: 0.40
Nodes (5): Consequences, D-100 — Worker commissioning is an append-only caveat-chain child, Decision, Rationale, Would change if

### Community 493 - "Store"
Cohesion: 0.40
Nodes (5): Consequences, D-101 — Receipt-bound, fail-closed worker dispatch, Decision, Rationale, Would change if

### Community 494 - "String"
Cohesion: 0.40
Nodes (5): Consequences, D-102 — Worker results relay under the delivery ack policy, Decision, Rationale, Would change if

### Community 495 - "Vec"
Cohesion: 0.40
Nodes (5): Consequences, D-103 — Catalog-owned literal egress declarations, Decision, Rationale, Would change if

### Community 496 - "Default"
Cohesion: 0.40
Nodes (5): Consequences, D-104 — Runtime skills are permitted on the gate-containment guarantee (revisits D-048), Decision, Rationale, Would change if

### Community 497 - "Path"
Cohesion: 0.40
Nodes (5): Consequences, D-105 — Kernel-bound skill-context attribution, Decision, Rationale, Would change if

### Community 498 - "PathBuf"
Cohesion: 0.40
Nodes (5): Consequences, D-106 — Digest-bound promotion previews, Decision, Rationale, Would change if

### Community 499 - "Result"
Cohesion: 0.40
Nodes (5): Consequences, D-006 — Identity is not authority, Decision, Rationale, Would change if

### Community 500 - "Self"
Cohesion: 0.40
Nodes (5): Consequences, D-007 — Task grant is the final runtime authority, Decision, Rationale, Would change if

### Community 501 - "String"
Cohesion: 0.40
Nodes (5): Consequences, D-011 — Approval must be digest-bound, Decision, Rationale, Would change if

### Community 502 - "Vec"
Cohesion: 0.40
Nodes (5): Consequences, D-012 — Audit stores private payloads by encrypted/hash reference, Decision, Rationale, Would change if

### Community 503 - "Option"
Cohesion: 0.40
Nodes (5): Consequences, D-013 — Dynamic behavior easy; dynamic authority hard, Decision, Rationale, Would change if

### Community 504 - "String"
Cohesion: 0.40
Nodes (5): Consequences, D-014 — Bootstrap/setup secrets bypass shell/model context, Decision, Rationale, Would change if

### Community 505 - "Timestamp"
Cohesion: 0.40
Nodes (5): Consequences, D-015 — Phase 1 should avoid final email send, Decision, Rationale, Would change if

### Community 506 - "Ulid"
Cohesion: 0.40
Nodes (5): Consequences, D-016 — Capability packs are candidate profiles, not live authority, Decision, Rationale, Would change if

### Community 510 - "Ulid"
Cohesion: 0.40
Nodes (5): Consequences, D-017 — Personas grant no authority, Decision, Rationale, Would change if

### Community 511 - "Error"
Cohesion: 0.40
Nodes (5): Consequences, D-018 — Routes are declarative artifacts, not kernel code, Decision, Rationale, Would change if

### Community 512 - "Option"
Cohesion: 0.40
Nodes (5): Consequences, D-020 — Railway/Docker/VPS are deployment targets, not core architecture, Decision, Rationale, Would change if

### Community 513 - "Result"
Cohesion: 0.40
Nodes (5): Consequences, D-025 — Rust/Tokio substrate: storage, audit chain, and secrets handling, Decision, Rationale, Would change if

### Community 514 - "Ulid"
Cohesion: 0.40
Nodes (5): Consequences, D-027 — Multi-provider model gateway with per-provider auth mode, Decision, Rationale, Would change if

### Community 515 - "Option"
Cohesion: 0.40
Nodes (5): Consequences, D-028 — Canonical artifact format is YAML+serde; digests are `sha256:` over canonical JSON, Decision, Rationale, Would change if

### Community 517 - "Ulid"
Cohesion: 0.40
Nodes (5): Consequences, D-029 — Gmail OAuth scopes: `readonly` + `compose`, send hard-denied at the gate, Decision, Rationale, Would change if

### Community 518 - "Value"
Cohesion: 0.40
Nodes (5): Consequences, D-030 — Telegram carries the entire owner-control UX for phases 1–3, Decision, Rationale, Would change if

### Community 519 - "Vec"
Cohesion: 0.40
Nodes (5): Consequences, D-031 — Docker Compose is the first reference deployment target, Decision, Rationale, Would change if

### Community 520 - "JoinHandle"
Cohesion: 0.40
Nodes (5): Consequences, D-032 — Kernel↔shell transport is HTTP/JSON with a per-task bearer token, Decision, Rationale, Would change if

### Community 521 - "Vec"
Cohesion: 0.40
Nodes (5): Consequences, D-033 — Action identifiers are exact-match dotted strings; unverified senders are audited and ignored, Decision, Rationale, Would change if

### Community 522 - "Option"
Cohesion: 0.40
Nodes (5): Consequences, D-034 — `email.create_draft` is the one canonical action id; the qualified PRD §10.2 spelling is dropped, Decision, Rationale, Would change if

### Community 524 - "String"
Cohesion: 0.40
Nodes (5): D-010 — Model calls with private context go through model gateway, Decision, Gateway responsibilities, Rationale, Would change if

### Community 527 - "Timestamp"
Cohesion: 0.40
Nodes (5): D-019 — Implement minimal slice first, not full agent OS, Decision, Minimal slice, Rationale, Would change if

### Community 528 - "Ulid"
Cohesion: 0.40
Nodes (4): Approach, Design: Backfill implemented capability specs, Dev-process restoration, Forward-looking requirement

### Community 529 - "D"
Cohesion: 0.40
Nodes (4): Alternatives considered, Approach, Design: Artifact lifecycle slice, Key decisions

### Community 530 - "CapabilityPack"
Cohesion: 0.40
Nodes (4): Canon and audited sites, Design, Routing, Storage

### Community 531 - "Error"
Cohesion: 0.40
Nodes (4): Authority-sensitive decisions, Design: Base/overlay compatibility, Failure behavior, Lifecycle state machine

### Community 532 - "F"
Cohesion: 0.40
Nodes (4): Acceptance Criteria, Global daily spend kill switch, What Changes, Why

### Community 533 - "Formatter"
Cohesion: 0.40
Nodes (4): Acceptance Criteria, Proposal: Implement the kernel task board, What Changes, Why

### Community 534 - "Into"
Cohesion: 0.40
Nodes (4): Acceptance Criteria, Proposal: Implement the skill artifact class, What Changes, Why

### Community 535 - "Result"
Cohesion: 0.40
Nodes (5): Delegation & containment, implement-briefcase-packing, implement-escalation-and-refusal, implement-worker-runtime, implement-worker-supervision

### Community 536 - "S"
Cohesion: 0.40
Nodes (5): implement-authority-equivalence-matcher, implement-seed-workflows, implement-skill-artifact-class, implement-workflow-state-machines, Skills & workflows

### Community 539 - "String"
Cohesion: 0.40
Nodes (5): implement-nerve-subscribers, implement-persona-binding-and-headless-lanes, implement-personality-seed, implement-reflection-miner, Reflection & product surface

### Community 540 - "Vec"
Cohesion: 0.40
Nodes (5): Requirement: Anti-pattern probes MUST fail on violating output and pass on clean output, Scenario: A committed row with a missing or corrupt file self-heals, Scenario: A violating sample trips its probe, Scenario: Clean output trips no probe, Scenario: Learned row and seeded receipt are atomic

### Community 541 - "Client"
Cohesion: 0.40
Nodes (5): Requirement: Master MUST commission workers and relay results as bus events, Scenario: Classified empty output channel is denied (`empty_declared_output_channels_fail_closed`), Scenario: Parent allows action but worker denies it (`worker_denied_outside_narrowed_allowlist`), Scenario: Worker commissioned and result consumed (`result_is_consumed_bus_event`), Scenario: Worker report action remains allowed (`worker_allowed_exact_report_action`)

### Community 542 - "ModelSwapManifest"
Cohesion: 0.40
Nodes (5): Requirement: Task grants MUST be explicit live authority objects, Scenario: Bound parameters are caveats, Scenario: Email reply drafter starts, Scenario: Root grant defaults, Scenario: Sub-grant is the sole presented authority

### Community 546 - "Policy"
Cohesion: 0.40
Nodes (4): devDependencies, @fission-ai/openspec, name, private

### Community 547 - "Result"
Cohesion: 0.83
Nodes (3): discard_staged_overlay_files(), load(), OverlayStartup

### Community 549 - "Vec"
Cohesion: 0.40
Nodes (4): Safe rules for changing behavior, The bet, The problem, What OpenSpine does not do on purpose

### Community 550 - "WorkflowManifest"
Cohesion: 0.67
Nodes (3): build_selection_token(), format_pending_message(), email_grant_binding()

### Community 551 - "PathBuf"
Cohesion: 0.83
Nodes (3): atomic_grant_and_briefcase_persists_both_on_success(), atomic_grant_and_briefcase_rolls_back_on_briefcase_failure(), minimal_briefcase()

### Community 552 - "Error"
Cohesion: 0.83
Nodes (3): Cli, main(), run()

### Community 553 - "D-031 — Docker Compose is the first reference deployment target"
Cohesion: 0.50
Nodes (3): Answer, Q: Trace the full runtime call path for a single incoming Telegram message from an authorized owner, all the way through to a TaskGrant being persisted in the kernel's SQLite store. List the modules/files involved, in call order, and identify the 2-3 modules that are most structurally central to this flow., Source Nodes

### Community 554 - "test_hooks.rs"
Cohesion: 0.50
Nodes (3): Blindspot pass, Guardrails, Steps

### Community 555 - "Option"
Cohesion: 0.50
Nodes (3): Brainstorm and prototypes, Guardrails, Steps

### Community 556 - "Path"
Cohesion: 0.50
Nodes (3): Change quiz, Guardrails, Steps

### Community 557 - "PathBuf"
Cohesion: 0.50
Nodes (3): Guardrails, Implementation notes, Steps

### Community 558 - "Result"
Cohesion: 0.50
Nodes (3): Guardrails, Implementation plan, Steps

### Community 559 - "Self"
Cohesion: 0.50
Nodes (3): Guardrails, Interview me, Steps

### Community 560 - "String"
Cohesion: 0.50
Nodes (3): Guardrails, Pitch packager, Steps

### Community 561 - "String"
Cohesion: 0.50
Nodes (3): Guardrails, Reference hunt, Steps

### Community 562 - "Vec"
Cohesion: 0.50
Nodes (3): Implementation, Tasks: Implement Day-2 Operations Contract, Verification

### Community 563 - "D-032 — Kernel↔shell transport is HTTP/JSON with a per-task bearer token"
Cohesion: 0.50
Nodes (3): Proposal: Implement workflow state machines, What Changes, Why

### Community 564 - "D-033 — Action identifiers are exact-match dotted strings; unverified senders are audited and ignored"
Cohesion: 0.50
Nodes (3): Connector reality hardening, What Changes, Why

### Community 565 - "Result"
Cohesion: 0.50
Nodes (4): Requirement: TaskGrant MUST carry an optional dormant thread_id, Scenario: Mutating thread_id invalidates the grant MAC, Scenario: TaskGrant with thread_id round-trips, Scenario: TaskGrant without thread_id deserializes as None

### Community 566 - "Self"
Cohesion: 0.50
Nodes (4): Requirement: Worker sub-grants MUST be offline-verifiable caveat-chain attenuations, Scenario: Offline verification of a multi-level chain (`offline_chain_verify_multi_level`), Scenario: Widening action is rejected (`child_cannot_widen_parent_action`), Scenario: Widening expiry is rejected (`child_cannot_widen_parent_expiry`)

### Community 567 - "head"
Cohesion: 0.83
Nodes (3): forbid(), require(), check-omp-ceremony.sh script

### Community 568 - "Value"
Cohesion: 0.50
Nodes (3): Crate map, The kernel/shell trust boundary, The pipeline

### Community 569 - "D-034 — `email.create_draft` is the one canonical action id; the qualified PRD §10.2 spelling is dropped"
Cohesion: 0.50
Nodes (3): A worked example: catching a real approval bypass, Reading the log, Why a decision log

### Community 570 - "Result"
Cohesion: 0.50
Nodes (3): Build and run the check script, Configure a real server, Try it

### Community 571 - "Option"
Cohesion: 0.50
Nodes (3): Backfilled, Deferred, on purpose, Shipped

### Community 572 - "Result"
Cohesion: 0.50
Nodes (3): Claims, Claims vs exclusions, honestly, What the current phases do not claim to defend against

### Community 574 - "Timestamp"
Cohesion: 0.50
Nodes (3): exclude, extends, include

### Community 578 - "Timestamp"
Cohesion: 0.67
Nodes (3): Requirement: Activation MUST require digest-bound owner approval, Scenario: A duplicate proposal for an already-active id and version is rejected, Scenario: Owner approves a proposal

### Community 579 - "String"
Cohesion: 0.67
Nodes (3): Requirement: Legacy overlay migration is discovery/quarantine only, Scenario: Legacy tap establishes ProducedBy before visibility, Scenario: Non-canonical legacy filename survives review

### Community 580 - "Timestamp"
Cohesion: 0.67
Nodes (3): Requirement: Personality seed artifacts MUST load as overlay learned artifacts with provenance, Scenario: Seed elements load into the registry with ProducedBy provenance, Scenario: Seed survives a kernel restart

### Community 581 - "Ulid"
Cohesion: 0.67
Nodes (3): Requirement: Personality seed seeding MUST be idempotent across boots, Scenario: A crash between file write and provenance row self-heals, Scenario: A second boot seeds nothing new

### Community 582 - "Value"
Cohesion: 0.67
Nodes (3): Requirement: Proposed artifacts MUST be schema-validated before persistence, Scenario: An unknown kind is rejected, Scenario: Malformed YAML is rejected

### Community 589 - "Vec"
Cohesion: 0.67
Nodes (3): Requirement: Audit schemas MUST reference private payloads by encrypted or hash refs, Scenario: Audit event carries aggregate stream coordinates, Scenario: Model request includes private email content

### Community 590 - "Into"
Cohesion: 0.67
Nodes (3): Requirement: EventEnvelope MUST carry an optional dormant thread_id, Scenario: EventEnvelope with thread_id round-trips, Scenario: EventEnvelope without thread_id deserializes as None

### Community 660 - "String"
Cohesion: 0.17
Nodes (20): artifact_identity_pairs(), artifact_version(), ArtifactRegistry, ArtifactSource, exclude_identity_pairs(), exclude_unbacked_persona_versions(), merge_registry(), overlay_filename() (+12 more)

## Knowledge Gaps
- **2131 isolated node(s):** `autoresearch.sh script`, `ArtifactNominatePayload`, `WorkerCommissionResponse`, `TokenResponse`, `GmailConnector` (+2126 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **940 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `AppState` connect `D-005 — Private-data shell must be contained` to `D-004 — Every effectful action goes through `gate()``, `event.rs`, `handle_owner_update`, `telegram.rs`, `content.d.ts`, `String`, `AppState`, `.sweep_expired_grants`, `ADDED Requirements`, `Requirements`, `Requirements`, `ADDED Requirements`, `action_catalog.rs`, `mod.rs`, `ADDED Requirements`, `MODIFIED Requirements`, `Option`, `Proposal: Backfill implemented capability specs`, `Proposal: Harden approval and budgets`, `Tasks: Implement artifact lifecycle slice`, `OpenSpine conventions`?**
  _High betweenness centrality (0.043) - this node is a cross-community bridge._
- **Why does `ActionId` connect `ADDED Requirements` to `Requirements`, `Proposal: Harden approval and budgets`, `event.rs`, `D-016 — Capability packs are candidate profiles, not live authority`, `telegram.rs`, `openspine-decision-log.md`, `D-029 — Gmail OAuth scopes: `readonly` + `compose`, send hard-denied at the gate`, `Option`, `action_catalog.rs`, `.fmt`, `post_action`, `AppState`, `Pitch packager`?**
  _High betweenness centrality (0.017) - this node is a cross-community bridge._
- **Why does `activate_approved_artifact()` connect `D-005 — Private-data shell must be contained` to `event.rs`, `D-021 — Email domain is broader than Gmail`, `Design: Authority composition`, `Reflection & product surface`, `ADDED Requirements`, `artifact_propose.rs`?**
  _High betweenness centrality (0.016) - this node is a cross-community bridge._
- **Are the 88 inferred relationships involving `handle_owner_update()` (e.g. with `activation_with_mutated_payload_is_denied()` and `approved_artifact_activates_into_registry_and_overlay()`) actually correct?**
  _`handle_owner_update()` has 88 INFERRED edges - model-reasoned connections that need verification._
- **Are the 83 inferred relationships involving `owner_update()` (e.g. with `activation_with_mutated_payload_is_denied()` and `approve_callback_update()`) actually correct?**
  _`owner_update()` has 83 INFERRED edges - model-reasoned connections that need verification._
- **Are the 77 inferred relationships involving `test_state()` (e.g. with `unregistered_known_action_returns_stub_shape()` and `artifact_revoke_dispatch_removes_rule_from_live_consultation()`) actually correct?**
  _`test_state()` has 77 INFERRED edges - model-reasoned connections that need verification._
- **What connects `autoresearch.sh script`, `ArtifactNominatePayload`, `WorkerCommissionResponse` to the rest of the system?**
  _2131 weakly-connected nodes found - possible documentation gaps or missing edges._