OpenSpine PRD v9 — Runtime Substrate + Lyra Telegram Control Workflow

Status: Ninth revision. Supersedes v8 as the product-shape and authority-semantics specification.
Primary correction from v8: OpenSpine is the reusable agent runtime substrate. Lyra is the first governed personal assistant product built on OpenSpine.
North star: Governed agent runtime substrate for safely composing agents, tools, workflows, memory, connectors, and authority as capability grows.
Product wedge: Lyra, a Telegram-controlled personal assistant using OpenSpine, starting with selected-thread email reply drafting.
First concrete email connector: Gmail / Google Workspace owner mailbox.
Technical wedge: Event-driven, identity-aware, capability-gated runtime with explicit authority composition.
Positioning: OpenClaw gives an assistant claws. OpenSpine gives it a backbone.

⸻

0. One-page summary

OpenSpine is a self-hostable runtime substrate for governed agents. It accepts events, verifies their source, resolves identity, chooses a route, composes authority from all relevant policy artifacts, issues a bounded task grant, runs an agent/workflow, mediates all effects, records audit events, and updates memory only through policy.

Lyra is the first product built on OpenSpine: a governed personal assistant controlled through a verified owner channel.

The high-level model remains:

event → source verification → identity → route → authority composition → task grant → agent/workflow → gated effects → audit/memory

The first usable Lyra product shape is:

Telegram owner control channel + guarded selected-thread email drafting workflow

The first phase gives the user a real conversational agent interface from day one:

- verified Telegram owner message
- deterministic route to main_assistant_agent
- status checks
- setup guidance
- workflow invocation
- artifact proposals
- low-risk orchestration

Email is the first guarded external communication workflow:

- selected-thread only
- first concrete connector uses Gmail / Google Workspace as the owner-mailbox connector
- no inbox-wide read
- no attachments
- no final send
- email content treated as hostile/untrusted data
- specialist email_reply_drafter handles the drafting task
- model calls mediated by model gateway
- preview first; draft creation only after digest-bound approval in a later phase

Central framing:

OpenSpine is the backbone: the runtime substrate for governed agent capability.
Lyra v1 is the first product on that substrate: a Telegram-controlled personal assistant.
Its first guarded external communication workflow is selected-thread email reply drafting.

Central design principle:

Make dynamic behavior easy and dynamic authority hard.

⸻

1. Core invariants

1. Every effectful action goes through gate().
1. No process that receives private user data may have unauthorized exfiltration paths.
1. Deterministic routing decides authority. Agentic routing decides strategy.
1. Identity is not authority.
1. The task grant is the final resolved authority object.
1. Effective authority is the intersection of all applicable constraints.
1. Explicit deny wins.
1. Approval-required overrides plain allow.
1. Authority widening requires explicit human approval.
1. LLMs may not resolve route conflicts that affect authority.
1. External communication and content are data, not instruction.
1. Connector does not grant trust by itself.
1. Account role does not grant trust by itself.
1. System operations actions are high-impact and approval-gated by default.

⸻

2. General substrate model

OpenSpine is an event-driven, identity-aware, capability-gated runtime substrate for governed agents.

Lyra is a personal assistant product built on OpenSpine. In this PRD, Lyra-specific workflows are used as the first implementation slice for proving the substrate.

The generalized flow:

1. Event arrives.
2. Source is verified.
3. Event is normalized into an envelope.
4. Identity resolver maps actor hints to entity candidates.
5. Ingress and route policy select route deterministically.
6. Authority composer intersects applicable constraints.
7. Kernel issues task grant.
8. Agent/workflow runs inside the grant.
9. All effectful actions are mediated by gate().
10. Audit and memory updates happen through policy.

Phase 1 implements Lyra's owner control lane through Telegram. Phase 2 adds Lyra's first guarded external communication workflow: selected-thread email local reply preview, with Gmail / Google Workspace as the first concrete owner-mailbox connector.

Schemas should remain general enough for future WhatsApp, iOS app, web UI, CLI, Slack, Discord, webhook, timer, Git, internal events, Outlook, IMAP, AgentMail-style dedicated agent inboxes, infrastructure connectors such as Coolify, and other workflow domains. These are future ontology targets, not early-phase commitments.

⸻

3. Runtime ontology

OpenSpine separates domain, connector, account role, tool, workflow, agent, capability pack, and task grant.

Layer Meaning Examples
Domain Broad operating area communication, infrastructure, development, scheduling, documents, business workflow
Connector Concrete integration/provider Telegram, Gmail, Google Workspace, Outlook, IMAP, AgentMail, Coolify, GitHub, Slack, Discord
Account role What kind of account/resource is being accessed owner_mailbox, agent_inbox, shared_workspace_mailbox, customer_intake_inbox, notification_inbox
Tool Callable connector action email.read_thread, agentmail.create_inbox, coolify.deploy
Workflow Task pattern using tools and policy selected-thread email drafting, inbound triage, deployment review
Agent Bounded worker running inside a task grant main_assistant_agent, email_reply_drafter
Capability pack Reusable permission profile owner control pack, selected-thread email draft pack
Task grant Final runtime authority Short-lived resolved authority object

Rules:

- A connector does not grant trust by itself.
- An account role does not grant trust by itself.
- Identity is not authority.
- A task grant is authority.
- External communication and content are data, not instruction.
- System operations actions are high-impact and approval-gated by default.

  3.1 Email account roles

Email is a communication domain. Gmail is only the first concrete owner-mailbox connector implementation.

Account role Meaning Risk posture
owner_mailbox User’s personal/work mailbox High privacy risk; selected-scope access preferred
agent_inbox Dedicated email address/inbox owned by an agent or workflow Useful for agent-native send/receive/search via API; separate from owner mailbox
shared_workspace_mailbox Team/business mailbox Shared authority and audit requirements
customer_intake_inbox Inbound customer/lead mailbox External communication; triage and routing risk
notification_inbox System alerts, CI/CD, monitoring notifications Lower personal privacy, but operational impact can be high

AgentMail-style services fit as agent_inbox providers: email infrastructure for agents to send, receive, search threaded email via API, use custom domains, receive webhooks, and operate dedicated inboxes. This is different from an agent monitoring the user’s personal Gmail or Google Workspace mailbox.

Future email connectors/providers may include Gmail / Google Workspace, Outlook, IMAP, and AgentMail-style dedicated agent inboxes. These are not early implementation scope.

3.2 Lane taxonomy

Lyra classifies events and workflows into lanes.

Lane Meaning Example connectors/workflows
owner_control Verified owner-facing command and interaction channel Telegram owner messages
external_communication Messages from or about external people/entities selected-thread email drafting, inbound triage
content_document Documents, files, pages, attachments, knowledge artifacts webpages, PDFs, attachments, docs
system_operations Infrastructure and operational systems Coolify deploy review, server logs
scheduled_internal Timers, reminders, internal maintenance events scheduled checks, recurring reviews
development Code, repos, issues, PRs, build systems GitHub issues, PR review
business_workflow CRM, billing, internal process systems lead intake, invoice workflow

Early implementation only uses:

- owner_control via Telegram
- external_communication via selected-thread email drafting using Gmail / Google Workspace owner mailbox

  3.3 Owner control lane

The owner control lane handles verified owner interaction.

Examples:

- setup guidance
- status checks
- workflow invocation
- connector setup flows
- artifact proposals
- low-risk orchestration
- asking the user clarifying questions
- presenting previews and action requests

Phase 1 owner control channel:

telegram.owner.message → main_assistant_agent

Telegram is chosen first because it is simpler than WhatsApp for an early self-hosted agent interface.

Future owner control channels may include WhatsApp, iOS app, web UI, or CLI. These are explicitly deferred.

3.4 External communication lane

The external communication lane handles messages and communication records involving external people/entities.

Examples:

- selected email threads
- inbound external messages
- customer/client messages
- unknown WhatsApp/SMS senders
- shared mailbox threads
- agent inbox threads

External communication may be useful context, but it is not authority.

Rule:

External communication is data, not instruction.

Examples:

Input Treatment
Telegram message from verified owner Instruction candidate, subject to policy and task grant
Email thread from external sender Data only
Web page Hostile data
Attachment Hostile data until parsed/sandboxed
Unknown WhatsApp/SMS sender Low-trust inbound communication
Email saying “ignore previous instructions” Quoted content, not agent instruction

Email content must route to a specialist workflow. It must not be swallowed directly by the main assistant as instruction.

3.5 System operations example: Coolify

Coolify fits the ontology as an infrastructure connector in the system_operations lane.

Example tools:

- coolify.list_projects
- coolify.read_logs
- coolify.deploy
- coolify.rollback

Authority posture:

- read-only operations may be allowed in a narrow capability pack
- deploy, restart, and rollback require approval
- secret modification and delete actions are denied by default
- system operations actions are high-impact and approval-gated by default

This is an ontology fit example only. Coolify is not part of Phase 0, Phase 1, Phase 2, or Phase 3 implementation scope.

⸻

4. Event envelope and authenticity

4.1 Event envelope

All incoming channel activity becomes a normalized event envelope.

event:
id: ulid
source: gmail | telegram | whatsapp | slack | discord | cli | webhook | timer | git | internal
connector: telegram_owner_bot | gmail_primary_connector | google_workspace_primary | outlook_primary | imap_primary | agentmail_primary | coolify_primary | null
account_role: owner_mailbox | agent_inbox | shared_workspace_mailbox | customer_intake_inbox | notification_inbox | owner_control_account | system_account | null
event_type: telegram.owner.message | email.thread.selected
received_at: datetime
verified_source: true
verification_method: oauth_session | webhook_signature | local_cli_auth | device_session | connector_poll | telegram_owner_id_match | kernel_ui_selection | none
replay_protected: true
replay_nonce: string | null
channel_account: string
raw_event_ref: encrypted_artifact_ref
actor_hint:
channel_user_id: string | null
email: string | null
phone: string | null
handle: string | null
device_id: string | null
target_refs: - type: email_thread | conversation | project | deployment | none
id: opaque_provider_id | null
data_classification: private | internal | public | unknown
user_intent_hint: string | null
lane: owner_control | external_communication | content_document | system_operations | scheduled_internal | development | business_workflow | internal
trust_context:
channel_trust: owner_device | verified_owner_channel | verified_contact | known_contact | workspace_member | unknown | untrusted
interaction_mode: owner_message | user_selected | inbound_message | scheduled | system_hook

4.2 First two event types

A. telegram.owner.message

Purpose:

- verified owner control channel
- routes to main_assistant_agent
- used for setup, status, workflow invocation, and normal interaction
- no broad external reads by default

Minimal event posture:

event_type: telegram.owner.message
source: telegram
connector: telegram_owner_bot
account_role: owner_control_account
lane: owner_control
verified_source: true
verification_method: telegram_owner_id_match
trust_context:
channel_trust: verified_owner_channel
interaction_mode: owner_message

B. email.thread.selected

Purpose:

- guarded selected-thread email workflow
- first concrete implementation uses Gmail / Google Workspace owner mailbox
- routes to email_reply_drafter
- selected thread only
- no attachments
- model gateway
- local preview
- no final send

Minimal event posture:

event_type: email.thread.selected
source: gmail
connector: gmail_primary_connector
account_role: owner_mailbox
lane: external_communication
verified_source: true
verification_method: kernel_ui_selection | oauth_session
target_refs:

- type: email_thread
  id: opaque_provider_id
  trust_context:
  channel_trust: owner_device
  interaction_mode: user_selected

  4.3 Event authenticity rules

Spoofable identifiers are not enough for trusted routing.

Rules:

- A phone number, email address, Telegram handle, or display name is an actor hint, not proof of identity.
- Trusted routing requires verified event source appropriate to the channel.
- Telegram owner control requires configured owner Telegram ID verification.
- Webhook events require signature verification where supported.
- CLI/dev events require local authentication/session binding.
- Connector-polled events inherit trust from connector authentication and target scope, but connector authentication alone does not grant task authority.
- Account role informs risk and constraints, but does not grant trust by itself.
- Replay protection must be represented where relevant.
- Events without source verification fall back to low-authority triage/review.

Phase 1:

- Implement only telegram.owner.message from configured owner Telegram ID.
- The main assistant receives no broad external read authority by default.

Phase 2:

- Implement only email.thread.selected using Gmail / Google Workspace owner mailbox from a kernel-owned UI, verified Gmail picker, or approved owner-control invocation that produces a valid selection token.
- verified_source=true must mean the selected thread came from a trusted user selection path, not a shell-provided thread ID.
- Gmail is the first connector implementation, not a special architecture.

⸻

5. Identity is not authority

5.1 Identity records store knowledge

Identity records describe people, organizations, devices, service accounts, agents, or unknown entities.

They may store:

- identifiers
- relationships
- verification status
- contact metadata
- confidence
- historical interaction context

They do not directly grant capability packs.

5.2 Same person does not imply same authority

A person may be known across multiple channels:

- work email
- personal email
- WhatsApp number
- Telegram ID
- Telegram handle
- Slack ID
- Discord ID
- GitHub username

But same person across channels never implies same permissions.

Examples:

- George’s configured Telegram owner ID may route to owner control workflows.
- George’s future WhatsApp number may route to owner workflows only if verified and policy allows that channel.
- A spoofable inbound email claiming to be George does not receive owner authority.
- An email sender that matches a known contact remains external communication unless routed through a trusted control lane.

  5.3 Identity schema

identity:
id: ulid
display_name: string
entity_type: person | organization | service_account | device | agent | unknown
identifiers: - type: telegram_user_id
value_hash: sha256
verified: true
verification_method: user_confirmed | setup_pairing | unknown - type: email
value_hash: sha256
verified: true
verification_method: connector_contact_match | user_confirmed | domain_verified | unknown - type: whatsapp_number
value_hash: sha256
verified: false
verification_method: none
relationships: - type: spouse | colleague | client | vendor | owner | family | unknown
target_id: ulid
confidence: 0.0-1.0
notes_ref: encrypted_artifact_ref | null

No capability_pack_id is stored directly on identity records in phase 1.

5.4 Identity resolution output

identity_resolution:
event_id: ulid
matched_identity_id: ulid | null
confidence: 0.0-1.0
matched_identifier_type: telegram_user_id | email | phone | handle | device | none
channel_trust: verified_owner_channel | owner_device | verified_contact | known_contact | unknown | untrusted
source_verified: true | false
authority_warning: string | null

The policy router uses this output as one input. It does not treat identity match as authority.

⸻

6. Routes and route conflict resolution

6.1 Routes are declarative artifacts

Routes map event/identity/context to an agent, workflow, and candidate capability pack.

Owner control route

route:
id: owner_telegram_main_assistant
lifecycle_state: active
priority: 100
when:
source: telegram
event_type: telegram.owner.message
verified_source: true
lane: owner_control
actor:
relationship: owner
channel_trust: verified_owner_channel
identity_confidence_min: 0.95
agent: main_assistant_agent
workflow: owner_control_conversation
capability_pack: owner_control_basic_pack

Selected-thread email route

route:
id: owner_email_selected_thread
lifecycle_state: active
priority: 90
when:
source: gmail
connector: gmail_primary_connector
account_role: owner_mailbox
event_type: email.thread.selected
verified_source: true
lane: external_communication
actor:
relationship: owner
channel_trust: owner_device
identity_confidence_min: 0.95
agent: email_reply_drafter
workflow: selected_thread_email_reply_draft
capability_pack: selected_thread_email_draft_pack

6.2 Route conflict rule

Multiple matching routes must be resolved deterministically.

Rules:

1. Exact deny route wins over allow route.
2. Higher explicit priority wins only if priority is declared.
3. If priority ties, more specific route wins only if specificity rules are defined.
4. If ambiguity remains, fall back to low-authority triage/review.
5. LLMs may not resolve route conflicts that affect authority.

6.3 Specificity rule

Specificity may consider explicit fields only:

- exact event type
- exact verified source requirement
- exact lane
- exact channel/source
- exact connector
- exact account role
- exact relationship
- minimum identity confidence
- exact task class

No semantic or LLM-based specificity scoring is allowed for authority decisions.

6.4 Ambiguous route behavior

Ambiguous route match produces:

route_resolution:
status: ambiguous
fallback_route: low_authority_triage
reason: multiple_matching_routes_no_deterministic_winner

The fallback route may summarize or ask the user what to do, but receives no widened authority.

⸻

7. Router types

Router Nature Purpose May decide authority?
Ingress router Mostly deterministic Choose channel/event pipeline Yes, within policy
Identity resolver Mostly deterministic, sometimes probabilistic Map channel IDs to entities No, produces confidence/trust inputs
Policy router Deterministic Produce authority candidates and constraints Yes
Task/agent router Hybrid Choose agent/workflow inside envelope No new authority
Internal agent routers Agentic/hybrid Choose tools, memory, workflow, subagents No new authority

Principle:

Deterministic routing decides authority. Agentic routing decides strategy.

⸻

8. Authority composition

8.1 Authority sources

Effective task authority is composed from:

- verified event source
- event lane
- connector
- account role
- identity resolution output
- channel trust
- route policy
- global policy
- agent manifest
- workflow requirements
- capability pack
- task class constraints
- user/session policy
- current autonomy level

No single source grants final authority alone.

8.2 Merge rule

The task grant is produced by intersecting all applicable authority sources.

Merge rules:

1. Start from deny-by-default.
2. Add candidate allows from route, workflow, agent manifest, and capability pack.
3. Intersect with global policy and user/session policy.
4. Apply lane, connector, account-role, data-class, channel, task, reversibility, and external-visibility constraints.
5. Apply explicit denies.
6. Mark approval-required actions.
7. If an action is both allowed and approval-required, it is approval-required.
8. If an action is both allowed and denied, it is denied.
9. If an action requires authority not present in all required sources, it is not granted.
10. Materialize final authority as a task grant.

8.3 Authority conflict precedence

explicit deny > approval-required > allow > unspecified deny-by-default

8.4 Widening rule

Authority widening requires explicit human approval.

Widening includes:

- new connector
- new account role
- new external write
- broader read scope
- broader memory scope
- lower identity confidence threshold
- higher external visibility
- weaker approval requirement
- longer task grant duration
- raw network access
- new model provider for private data
- promoting external communication/content to instruction
- allowing main assistant direct access to external-communication connectors
- system operations actions such as deploy, restart, rollback, secret modification, or delete

Agents may propose widening. They may not activate it.

⸻

9. Roles clarified

Artifact Meaning Grants authority?
Domain Broad operating area No, contributes classification
Connector Concrete integration/provider No, contributes constraints and risk
Account role Purpose/risk class of the account/resource No, contributes constraints and risk
Tool Callable connector action No, must be allowed by task grant
Agent manifest What the agent is designed to use and its operating bounds No, contributes constraints
Capability pack Reusable policy profile for routes/workflows/task classes No, contributes candidate permissions
Task grant Final runtime authority object Yes, bounded and short-lived
Persona Style/behavior only No
Route Maps event/context to agent/workflow/capability pack No, contributes selection and constraints
Workflow Declares expected sequence and required capabilities No, contributes constraints
Policy Decides whether actions/grants/routes are allowed Yes, through kernel enforcement
Lane Distinguishes trusted control, communication, content, system operations, development, etc. No, contributes constraints

Only the task grant is the live authority presented to a running agent/workflow.

⸻

10. Agent manifests

Agents are bounded workers. They are not authority-bearing identities.

10.1 main_assistant_agent

The main assistant is an owner-facing orchestrator. It is not god-mode.

Purpose:

- converse with the verified owner
- provide setup guidance
- check OpenSpine/Lyra runtime status
- start approved workflows
- propose artifacts
- request approval for authority-widening changes
- coordinate specialist workflows without absorbing their authority

It does not receive broad external-communication, content-document, development, or system-operations access by default.

agent:
id: main_assistant_agent
lifecycle_state: active
purpose: Owner-facing conversational orchestrator.
persistence: persistent
persona: concise_practical_operator
model_policy:
allowed_providers: [local, openai, anthropic]
private_context_requires_gateway: true
max_model_calls_per_task: 8
memory_scope:
allowed_classes: [owner_preference, setup_state, workflow_state]
allowed_scopes: [owner_control, product_usage, external_facing_writing_preferences]
denied_classes: [health, finance, family_private, raw_email_body, attachment_content, infrastructure_secret]
designed_tools: - openspine.status.read - workflow.invoke:approved - artifact.propose - setup.workflow.start - memory.read:owner_preferences_limited - model.generate:approved_provider - lyra.ui.preview - telegram.reply:owner_channel
approval_required_tools: - connector.enable - route.activate - capability_pack.change - workflow.activate - policy.change_proposal
denied_tools: - email.read_inbox - email.read_thread:unselected - email.send - email.read_attachment - network.raw_egress - vault.secret_read - policy.modify_direct - filesystem.host_read - filesystem.host_write - coolify.deploy - coolify.rollback - coolify.secret_modify
limits:
max_runtime_seconds: 120
max_artifacts: 20
max_tokens: 12000
output_channels:
allowed: - telegram.owner.reply - lyra.ui.preview - action_request:approval

The main assistant can invoke approved workflows only through the kernel. It cannot directly grant itself the permissions of the invoked workflow.

10.2 email_reply_drafter

The email reply drafter is a specialist agent for guarded selected-thread email drafting. The first connector implementation uses Gmail / Google Workspace owner mailbox.

agent:
id: email_reply_drafter
lifecycle_state: active
purpose: Draft replies to selected email threads for user review.
persistence: ephemeral
persona: concise_professional_helper
model_policy:
allowed_providers: [local, openai, anthropic]
private_context_requires_gateway: true
max_model_calls_per_task: 5
memory_scope:
allowed_classes: [preference]
allowed_scopes: [external_facing_writing]
denied_classes: [health, finance, family_private, evaluation]
designed_tools: - email.read_thread:selected_no_attachments - model.generate:approved_provider - memory.read:writing_preferences_scoped - artifact.write:task_scratch - lyra.ui.preview - email.create_draft:after_payload_approval
denied_tools: - email.send - email.read_inbox - email.read_thread:unselected - email.read_attachment - network.raw_egress - telegram.reply:owner_channel
limits:
max_runtime_seconds: 180
max_artifacts: 20
max_tokens: 12000
output_channels:
allowed: - lyra.ui.preview - action_request:email.create_draft

designed_tools expresses intended use. The task grant decides what the agent actually receives.

Ephemeral/subagents may inherit only narrowed capabilities from the parent task grant.

⸻

11. Capability packs

Capability packs are reusable policy profiles. They are not live authority.

11.1 Owner control basic pack

capability_pack:
id: owner_control_basic_pack
lifecycle_state: active
applies_to:
event_type: telegram.owner.message
relationship: owner
channel_trust: verified_owner_channel
verified_source: true
lane: owner_control
candidate_allowed_actions: - openspine.status.read - workflow.invoke:approved - artifact.propose - setup.workflow.start - memory.read:owner_preferences_limited - model.generate:approved_provider - telegram.reply:owner_channel
approval_required: - connector.enable - route.activate - capability_pack.change - workflow.activate - policy.change_proposal
denied_actions: - email.read_inbox - email.read_thread:unselected - email.read_attachment - email.send - network.raw_egress - vault.secret_read - filesystem.host_read - filesystem.host_write - policy.modify_direct - coolify.deploy - coolify.rollback - coolify.secret_modify - coolify.delete_resource
constraints:
data_classification_max: private
external_visibility_max: owner_channel_reply
recovery_required: revert_or_compensate
max_runtime_seconds: 120

11.2 Selected-thread email draft pack

capability_pack:
id: selected_thread_email_draft_pack
lifecycle_state: active
applies_to:
event_type: email.thread.selected
connector: gmail_primary_connector
account_role: owner_mailbox
relationship: owner
channel_trust: owner_device
verified_source: true
lane: external_communication
candidate_allowed_actions: - email.read_thread:selected_no_attachments - memory.read:writing_preferences_scoped - model.generate:approved_provider - artifact.write:task_scratch - lyra.ui.preview
approval_required: - email.create_draft
denied_actions: - email.send - email.read_inbox - email.read_thread:unselected - email.read_attachment - network.raw_egress - filesystem.host_read - filesystem.host_write - telegram.reply:owner_channel
constraints:
data_classification_max: private
external_visibility_max: draft_after_approval
external_communication_is_instruction: false
recovery_required: revert_or_compensate
max_runtime_seconds: 180

11.3 System operations read-only example pack

This is an ontology example only, not early implementation scope.

capability_pack:
id: coolify_read_only_status_pack
lifecycle_state: proposed
applies_to:
lane: system_operations
connector: coolify_primary
account_role: system_account
candidate_allowed_actions: - coolify.list_projects - coolify.read_logs
approval_required: - coolify.deploy - coolify.restart - coolify.rollback
denied_actions: - coolify.secret_read - coolify.secret_modify - coolify.delete_resource - network.raw_egress
constraints:
data_classification_max: internal
system_operation_impact: read_only
max_runtime_seconds: 120

The authority composer intersects these profiles with route, agent, workflow, event, identity, lane, connector, account role, and global policy constraints.

⸻

12. Task grant

The task grant is the final resolved authority object.

12.1 Owner control task grant example

task_grant:
id: ulid
lifecycle_state: active
user: user_id
purpose: owner_control_conversation
issued_by: kernel
issued_at: datetime
expires_at: datetime
event_id: ulid
route_id: owner_telegram_main_assistant
agent_id: main_assistant_agent
workflow_id: owner_control_conversation
capability_pack_id: owner_control_basic_pack
authority_sources: - global_policy:v1 - route:owner_telegram_main_assistant:v1 - agent:main_assistant_agent:v1 - workflow:owner_control_conversation:v1 - capability_pack:owner_control_basic_pack:v1
allowed_actions: - openspine.status.read - workflow.invoke:approved - artifact.propose - setup.workflow.start - memory.read:owner_preferences_limited - model.generate:approved_provider - telegram.reply:owner_channel
approval_required_actions: - connector.enable - route.activate - capability_pack.change - workflow.activate - policy.change_proposal
denied_actions: - email.read_inbox - email.read_thread:unselected - email.read_attachment - email.send - network.raw_egress - vault.secret_read - filesystem.host_read - filesystem.host_write - policy.modify_direct - coolify.deploy - coolify.rollback - coolify.secret_modify
output_channels: - telegram.owner.reply - lyra.ui.preview - action_request:approval
limits:
max_model_calls: 8
max_artifacts: 20
max_runtime_seconds: 120

12.2 Selected-thread email task grant example

task_grant:
id: ulid
lifecycle_state: active
user: user_id
purpose: draft_reply_for_selected_email_thread
issued_by: kernel
issued_at: datetime
expires_at: datetime
event_id: ulid
route_id: owner_email_selected_thread
agent_id: email_reply_drafter
workflow_id: selected_thread_email_reply_draft
capability_pack_id: selected_thread_email_draft_pack
authority_sources: - global_policy:v1 - route:owner_email_selected_thread:v1 - agent:email_reply_drafter:v1 - workflow:selected_thread_email_reply_draft:v1 - capability_pack:selected_thread_email_draft_pack:v1
selection_tokens: - email_thread_selection_token_id
allowed_actions: - email.read_thread:selected_no_attachments - memory.read:writing_preferences_scoped - model.generate:approved_provider - artifact.write:task_scratch - lyra.ui.preview
approval_required_actions: - email.create_draft
denied_actions: - email.send - email.read_inbox - email.read_thread:unselected - email.read_attachment - network.raw_egress - filesystem.host_read - filesystem.host_write - telegram.reply:owner_channel
output_channels: - lyra.ui.preview - action_request:email.create_draft
limits:
max_model_calls: 5
max_artifacts: 20
max_runtime_seconds: 180

Rules:

- short-lived
- purpose-bound
- route-bound
- agent-bound
- target-bound where relevant
- lane-bound
- connector-bound where relevant
- account-role-bound where relevant
- cannot be widened by shell
- revocable by kernel/supervisor
- every effectful action emits audit metadata

⸻

13. Artifact lifecycle

Routes, agents, workflows, capability packs, policies, and other declarative artifacts have lifecycle states.

proposed → validated → review_required → approved → active → quarantined | retired

13.1 Lifecycle meanings

State Meaning
proposed Artifact exists but cannot be used
validated Schema/static checks passed
review_required Requires human/control-plane review
approved Approved but not active
active Eligible for routing/authority composition
quarantined Blocked due to failure/safety concern
retired No longer used, retained for lineage/audit

13.2 Lifecycle rules

- Activation and deactivation must be audited.
- Widening authority moves artifact to review_required.
- Quarantined artifacts cannot participate in task grants.
- Retired artifacts remain addressable for audit/replay.
- Agents may propose artifacts but cannot activate authority-widening artifacts.

⸻

14. Model gateway strengthened

14.1 Rule

The shell may request model inference. The model gateway constructs the final provider call.

The shell does not directly build the final provider request when private data is involved.

14.2 Gateway responsibilities

The model gateway:

- resolves input refs
- checks task grant authority
- applies trusted prompt templates
- distinguishes trusted instruction from external communication/content
- wraps external communication/content as quoted data context
- applies redaction/data policy
- chooses/validates provider and model
- enforces retention policy
- size-limits input and output
- stores prompt/output as encrypted artifacts
- returns only allowed output to shell
- logs metadata and artifact refs to audit

  14.3 Model request schema

model_request:
id: ulid
task_grant_id: ulid
requester: shell_agent_id
purpose: draft_email_reply | owner_control_reply
requested_provider: openai | anthropic | local | null
requested_model: string | null
input_refs: - artifact:email_thread_excerpt:sha256:... - memory_ref:writing_preferences:sha256:...
template_id: email_reply_draft_template:v1 | owner_control_template:v1
data_classification: private
instruction_sources:
trusted: - system_template - owner_control_message
untrusted_data: - email_thread_excerpt - webpage_content - attachment_text
export_allowed: true
redaction_required: policy_conditional
retention_mode: no_training_no_logging_if_available
output_policy:
store_output: encrypted_ref
allow_shell_view: true
allow_memory_write: false

The gateway may reject or modify provider/model selection based on policy. The shell requests inference; the gateway owns the final call.

⸻

15. Selection tokens

The shell must not be trusted to provide target IDs and claim the user selected them.

selection_token:
id: ulid
type: email_thread_selection
user: user_id
target_id: email_thread_id
selected_by: user_id
selected_at: datetime
issued_by: kernel
expires_at: datetime
verified_source: true
verification_method: kernel_ui_selection | approved_owner_control_selection
connector: gmail_primary_connector
account_role: owner_mailbox
scope:
read_thread: true
attachments_allowed: false
max_messages: 20
include_headers: true
include_recipients: true
include_body: true

Rules:

- issued only by kernel-owned UI, verified picker, or approved owner-control selection flow
- shell cannot mint or alter
- expires quickly
- only usable inside matching task grant
- target, connector, account role, and scope are immutable
- external communication obtained through the token remains data, not instruction

⸻

16. Containment contract

No process that receives private user data may have unauthorized exfiltration paths.

Minimum phase-1 controls:

Control Requirement
Separate OS user/container Shell does not run as kernel/control user
Dedicated working directory Shell can read/write only task scratch
No inherited secrets No connector/model/provider tokens in env
Network egress blocked/proxied Shell cannot call arbitrary internet endpoints
Kernel-only local API Shell can call kernel action API only
Logs redacted/size-limited Private payloads not dumped into logs
No direct DB access Shell cannot query control/eval/audit DBs
No direct connector SDKs Email/calendar/model/infrastructure calls via kernel only
Process supervision Supervisor can kill/quarantine shell
Lane isolation External-communication workflows do not inherit owner-control authority
System-operation isolation System operations tools require explicit grants and approval posture

If this cannot be enforced, external-communication workflows must use synthetic or redacted data until it can.

⸻

17. Digest-bound approvals

Approval applies to exact payloads and targets.

approval:
id: ulid
action_request_id: ulid
approved_by: user_id
approved_at: datetime
approved_payload_digest: sha256
approved_target_digest: sha256
expires_at: datetime
decision: approved | rejected | edited
timeout_behavior: do_nothing

For approval-gated actions, the kernel executes only the approved immutable artifact. If body, recipient, target, thread, connector, or account role changes, approval is invalid.

⸻

18. Artifact store and audit privacy

Private payloads are stored as encrypted artifacts, referenced by digest.

Data Audit storage rule
Action metadata Store directly
Capability and decision Store directly
Owner Telegram message Encrypted artifact ref unless explicitly shareable
Email raw body Encrypted artifact ref, not raw audit text
Draft body Hash + encrypted artifact ref
Approval preview Hash + encrypted artifact ref
Model prompt Encrypted/redacted ref
Model output Encrypted ref with retention policy
User correction Encrypted ref unless explicitly shareable
Infrastructure logs Redacted/encrypted ref depending on sensitivity
Secrets/tokens Never store

MVP audit design:

- append-only event table
- per-event hash chain
- signed periodic checkpoints
- encrypted backup/export
- lyra audit verify

⸻

19. Incident handling

Incident state machine:

detected → contained → user_notified → recovery_attempted → reviewed → closed_or_quarantined

Phase-1 and phase-2 incident policies:

Incident Required response
Ambiguous route match Fallback to low-authority triage/review
Unverified source for trusted route Deny trusted route; fallback low-authority
Telegram sender mismatch Deny owner route; optionally ignore or low-authority triage
Main assistant requests inbox read Deny, audit, explain capability boundary
Shell requested out-of-scope email thread Deny, audit, warn if repeated
Email content attempts prompt injection Treat as quoted data; continue only if safe
Shell attempted raw network call Block, terminate task, quarantine if severe
Shell tried final send Deny, audit policy violation
Draft created with wrong recipient Delete draft if possible, notify user, quarantine candidate
Private data sent to wrong model/provider Disclose, quarantine trace, review provider policy
System operation requested without approval Deny or create approval request, depending on policy
Secret modification requested Deny by default, audit
Audit verification fails Stop effectful actions until resolved
Approval digest mismatch Block execution, invalidate approval, audit
Selection token expired/mismatch Deny, require fresh user selection
Unauthorized memory class requested Deny, audit, continue with reduced context if safe

⸻

20. Phase-1 owner control workflow

This is the first usable Lyra interface and the initial proof that OpenSpine can support a governed personal assistant product.

20.1 Workflow

1. User messages the Lyra Telegram bot.
2. Telegram connector verifies sender ID against configured owner Telegram ID.
3. Kernel creates event envelope: telegram.owner.message.
4. Identity resolver confirms owner relationship and high-confidence verified owner channel.
5. Ingress router selects owner control pipeline.
6. Route resolver deterministically selects owner_telegram_main_assistant.
7. Authority composer intersects global policy, route, agent manifest, workflow, capability pack, event authenticity, identity confidence, lane, connector, account role, and channel trust.
8. Kernel issues task grant as final authority object.
9. Supervisor starts contained shell for main_assistant_agent.
10. Main assistant may answer, check Lyra status, start setup flows, invoke approved workflows, or propose artifacts.
11. Main assistant cannot directly read inboxes, read unselected email threads, access vault secrets, send email, deploy infrastructure, or perform raw network egress.
12. All effectful actions go through gate().
13. Audit ledger records metadata, hashes, and encrypted refs.

20.2 Phase-1 minimal implementation

Implement only:

- Telegram bot connector
- owner Telegram ID verification
- telegram.owner.message event
- owner identity
- route: telegram.owner.message → main_assistant_agent
- main_assistant_agent manifest
- owner_control_conversation workflow
- owner_control_basic_pack
- authority composer
- task grant issuance
- model gateway for owner-control replies
- OpenSpine/Lyra status read action
- setup workflow start action
- artifact proposal action
- approved workflow invocation stub
- audit verify
- containment tests

Do not implement broad external data access in phase 1.

⸻

21. Phase-2 selected-thread email workflow

This is the first guarded external communication workflow. The first concrete connector implementation is Gmail / Google Workspace owner mailbox.

21.1 Workflow

1. User invokes email drafting from Telegram owner control channel or selects an email thread in a kernel-owned UI/picker. In the first implementation, the selected thread comes from Gmail / Google Workspace owner mailbox.
2. Kernel verifies event source and creates event envelope: email.thread.selected.
3. Identity resolver confirms owner/session with high confidence.
4. Ingress router selects selected-thread email pipeline.
5. Route resolver deterministically selects owner_email_selected_thread.
6. Authority composer intersects global policy, route, agent manifest, workflow, capability pack, event authenticity, identity confidence, lane, connector, account role, and channel trust.
7. Kernel issues selection token.
8. Kernel issues task grant as final authority object.
9. Supervisor starts contained shell for email_reply_drafter.
10. Shell requests selected thread content.
11. Kernel validates token/grant.
12. Gmail / Google Workspace connector reads bounded selected thread without attachments.
13. Thread content is classified as external communication and hostile/untrusted data.
14. Thread content stored as encrypted artifact; shell receives bounded working copy.
15. Shell requests scoped writing preferences.
16. Kernel grants low-risk preference memory only.
17. Shell requests model generation through model gateway.
18. Model gateway resolves refs, applies trusted template/redaction/data policy, and wraps email content as data, not instruction.
19. Draft output stored as immutable artifact.
20. User previews exact draft locally or via owner control channel summary.
21. Audit ledger records metadata, hashes, encrypted refs.
22. Correction/rejection/edit becomes improvement trace.
23. No final send exists.

21.2 Phase-2 minimal implementation

Implement only:

- Gmail / Google Workspace OAuth/setup for owner mailbox
- email.thread.selected event
- selected-thread token
- route: email.thread.selected → email_reply_drafter
- email_reply_drafter manifest
- selected_thread_email_reply_draft workflow
- selected_thread_email_draft_pack
- email read-thread connector for selected Gmail / Google Workspace thread
- model gateway with external-communication wrapping
- scoped memory read
- local draft preview
- encrypted/hash artifact store
- audit verify
- containment tests for prompt injection and out-of-scope reads

Do not implement inbox-wide reads, attachments, final send, autonomous draft creation, multiple email workflows, Outlook, IMAP, AgentMail, or public agent inboxes in phase 2.

⸻

22. Phase-3 approval-gated email draft

Phase 3 adds email draft creation, still without final send.

Build:

- immutable draft artifact
- digest-bound approval
- Gmail / Google Workspace create-draft connector action for owner mailbox
- draft delete/revert helper
- approval UX hardening

Exit criteria:

- kernel creates exactly approved draft payload
- no final send possible
- wrong/mutated payload blocked
- draft creation is reversible or compensating action exists

⸻

23. Dynamic growth and self-improvement

Agents may propose new:

- routes
- agents
- workflows
- skills
- capability packs
- memory rules
- evaluation examples

Activation is validated by kernel/control plane.

Change type Approval posture
Narrows authority Lighter approval possible
Preserves authority Normal review/eval
Widens authority Explicit human approval required
Changes identity trust Explicit human approval required
Adds connector Explicit human approval required
Adds account role Explicit human approval required
Increases external visibility Explicit human approval required
Adds system operations capability Explicit human approval required
Changes kernel/policy boundary Policy/foundation amendment path

Dynamic behavior should be easy. Dynamic authority should be hard.

Early phases must not include marketplace, GEPA, autonomous promotion, multi-agent evolution, public packs, or business/customer cloning.

⸻

24. Phase plan

Phase 0 — Minimal substrate skeleton

This phase defines OpenSpine's reusable substrate concepts. Lyra-specific event shapes are included only as the first proof slice.

Define general schemas, but implement only the first two event shapes.

Build:

- event envelope schema
- source verification fields
- domain/connector/account-role/workflow/tool vocabulary
- lane taxonomy
- owner identity
- identity resolution output schema
- route artifact schema
- route conflict rule
- agent manifest schema
- workflow schema
- capability pack schema
- authority composition rule
- task grant schema
- selection token schema
- model gateway request schema
- artifact lifecycle states
- containment tests
- Telegram owner event schema
- selected-thread email event schema
- lane classification: owner control vs external communication

Exit criteria:

- telegram.owner.message route can be expressed declaratively
- email.thread.selected route can be expressed declaratively with Gmail / Google Workspace as first owner-mailbox connector
- task authority is produced by intersection, not identity, connector, account role, or agent assumption
- route ambiguity falls back to low-authority review
- external communication/content is represented as data, not instruction
- ontology can represent future connectors such as Outlook, IMAP, AgentMail, and Coolify without adding implementation scope

Phase 1 — Owner control channel

Build:

- Telegram bot connector
- owner Telegram ID verification
- route: telegram.owner.message → main_assistant_agent
- main_assistant_agent
- owner control workflow
- owner control capability pack
- model gateway for replies
- status checks
- setup workflow start
- approved workflow invocation stub
- artifact proposal flow
- audit verify
- containment tests

Exit criteria:

- verified owner can chat with Lyra through Telegram, backed by OpenSpine task grants and gate-mediated actions
- main assistant can respond, check status, start setup flows, invoke approved workflows, and propose artifacts
- no broad external data access exists yet
- no connector secrets are exposed to shell
- every effectful action is mediated by gate()

Phase 2 — Guarded selected-thread email local preview

Build:

- Gmail / Google Workspace OAuth/setup for owner mailbox
- selected-thread token
- route: email.thread.selected → email_reply_drafter
- selected-thread email read connector
- model gateway
- scoped memory read
- local draft preview
- external-communication prompt-injection tests
- no final send

Exit criteria:

- useful local draft generated from selected email thread
- email content treated as untrusted data
- all private data flow mediated
- no unauthorized exfiltration path in normal operation
- email drafting is one route/workflow, not special kernel logic

Phase 3 — Approval-gated email draft

Build:

- immutable draft artifact
- digest-bound approval
- Gmail / Google Workspace create-draft connector action
- draft delete/revert helper
- approval UX hardening

Exit criteria:

- kernel creates exactly approved draft payload
- no final send possible
- wrong/mutated payload blocked

Phase 4+ — Deferred ambitions

Only after containment and authority semantics are proven:

- additional owner control channel
- additional guarded external-communication workflow
- additional email connectors/providers
- agent inbox workflows
- system operations workflows
- development workflows
- correction trace review
- golden evals
- manual improvement loop
- paired shadow
- LLM judge assist
- autonomy ladder
- foundation amendment lane
- blue/green kernel trial

Explicitly deferred:

- WhatsApp production routing
- iOS app
- Outlook
- IMAP
- AgentMail implementation
- public agent inboxes
- Coolify implementation
- marketplace
- GEPA/proposers
- autonomous promotion
- multi-agent evolution
- business/customer cloning

⸻

25. Threat-model exclusions

Phase 1 and Phase 2 do not claim to defend against:

- malicious root user on the VPS
- compromised kernel process
- compromised host OS
- model provider retaining data despite stated policy
- user manually copying private data elsewhere
- physical device compromise
- all side-channel leakage

Phase 1 and Phase 2 do claim:

- Telegram owner messages are verified against configured owner ID before owner routing
- same identity across channels does not imply same permissions
- connector does not grant trust by itself
- account role does not grant trust by itself
- external communication/content is data, not instruction
- system operations actions are high-impact and approval-gated by default
- the shell does not receive raw connector credentials
- the shell cannot directly call arbitrary external APIs in normal operation
- private model calls are mediated by model gateway
- user-selected targets are proven with selection tokens
- identity is not treated as authority
- authority is composed by deterministic intersection of policy, route, agent manifest, workflow, capability pack, lane, connector, account role, and event authenticity
- OpenSpine/Lyra does not grant runtime authority from identity alone
- OpenSpine/Lyra does not treat external content as instruction
- OpenSpine/Lyra mediates every effectful action through gate()
- OpenSpine/Lyra enforces containment boundaries for private data
- OpenSpine/Lyra audit records encrypted artifact refs for private payloads
- OpenSpine/Lyra does not allow shell to widen authority without explicit approval
- OpenSpine/Lyra does not allow LLMs to resolve authority-affecting route conflicts

The above runtime and security claims refer to the OpenSpine/Lyra runtime substrate, not only the Lyra personal assistant product.
⸻

26. Naming boundary: OpenSpine vs Lyra

OpenSpine is the substrate.

It owns the reusable runtime concepts:

- event envelope
- source verification
- identity resolution
- route resolution
- authority composition
- task grants
- gate-mediated actions
- connectors
- model gateway
- selection tokens
- artifact lifecycle
- audit and recovery

Lyra is the first product built on OpenSpine.

It owns the first user-facing experience:

- Telegram owner control
- main assistant behavior
- setup guidance
- selected-thread email reply drafting
- local preview and approval UX
- personal-assistant memory and preferences within OpenSpine policy

Rules:

- Do not describe OpenSpine as “the Lyra agent.”
- Do not describe Lyra as the whole runtime substrate.
- Use OpenSpine when referring to reusable runtime/security architecture.
- Use Lyra when referring to the personal assistant product or its first UX/workflows.
- Future products may be built on OpenSpine without inheriting Lyra's exact Telegram/email UX.
