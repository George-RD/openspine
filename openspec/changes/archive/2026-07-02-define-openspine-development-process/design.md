# Design: OpenSpine development process

## Context

OpenSpine is a governed runtime substrate for composing agents, tools, workflows, memory, connectors, routes, authority, and audit.

Lyra is the first personal assistant product built on OpenSpine.

OpenSpec is used to manage development changes to OpenSpine and Lyra.

The key design boundary is:

```text
OpenSpec controls how OpenSpine changes are proposed and implemented.

OpenSpine controls how runtime agents receive authority and perform effects.
```

## Design goals

- Keep OpenSpec useful without making it part of runtime authority.
- Make future OpenSpine changes reviewable and bounded.
- Prevent agents from turning the PRD into broad, unsafe implementation work.
- Ensure security-sensitive changes include explicit verification.
- Preserve the PRD and decision log as high-authority design context.

## Development lifecycle

OpenSpine development uses the standard OpenSpec lifecycle:

```text
explore → propose → spec/design/tasks → apply → archive
```

Interpretation for OpenSpine:

| Stage | Meaning for OpenSpine |
|---|---|
| Explore | Think through an architecture or implementation slice without writing runtime code |
| Propose | Define the change, scope, risks, non-goals, and affected layer |
| Specs | Define testable behavior and safety requirements |
| Design | Explain the technical approach, trade-offs, and rejected alternatives |
| Tasks | Break implementation and verification into small steps |
| Apply | Implement only the scoped tasks |
| Archive | Preserve rationale and merge accepted specs |

## Layer classification

Every proposal classifies the affected layer.

| Layer | Meaning | Examples |
|---|---|---|
| OpenSpine core | Runtime substrate behavior | event envelope, routes, task grants, gate, model gateway, audit |
| Lyra product | First assistant product behavior | Telegram UX, assistant persona, selected-thread email drafting UX |
| Both | Product workflow proving substrate behavior | Telegram route backed by task grant |
| Development tooling | Repo process and agent workflow | OpenSpec skills, config, Graphify instructions |

This classification prevents naming drift and scope confusion.

## Authority-sensitive changes

Some changes are ordinary implementation work. Others can widen or weaken authority.

A change is authority-sensitive if it affects:

- event authenticity;
- identity resolution;
- deterministic route resolution;
- capability packs;
- task grants;
- gate();
- approval requirements;
- connector access;
- account roles;
- memory scope;
- model provider access;
- network access;
- filesystem access;
- system operations;
- audit;
- containment.

Authority-sensitive changes require:

- explicit proposal marking;
- design section covering authority impact;
- verification tasks;
- decision-log check.

## OpenSpec artifacts are not runtime artifacts

OpenSpec artifacts can describe proposed runtime artifacts.

They cannot activate runtime artifacts.

Example:

```text
openspec/changes/add-slack-connector/specs/...
```

may describe a future Slack connector.

It does not create a live connector, live route, live capability pack, or live task grant.

Activation belongs to OpenSpine runtime/control-plane lifecycle, not OpenSpec alone.

## Future custom schema

The current repo uses `spec-driven`.

That is acceptable for now.

A future OpenSpine-specific schema may be useful:

```text
research → decision → proposal → specs → design → tests → tasks → implementation
```

Do not create it yet.

Reason:

- The default `spec-driven` schema is already enough to start.
- Premature schema design could delay implementation.
- OpenSpine should first prove a few development slices using the default process.

## Recommended first implementation slices

After this process change, create separate changes in this order.

### 1. `define-core-runtime-schemas`

Purpose:

Define schema files for the core OpenSpine concepts without implementing live behavior.

Includes:

- event envelope;
- identity resolution output;
- route artifact;
- route resolution result;
- agent manifest;
- workflow manifest;
- capability pack;
- task grant;
- action request;
- approval record;
- selection token;
- model request;
- audit event.

Why first:

It converts the PRD into typed artifacts and reduces ambiguity before code.

### 2. `implement-authority-composition`

Purpose:

Implement the deny-by-default authority composer and task-grant construction.

Includes:

- source inputs;
- merge rules;
- explicit deny precedence;
- approval-required precedence;
- intersection semantics;
- tests for conflicting authorities.

Why second:

Task grants are the central runtime authority object.

### 3. `implement-gate-action-api`

Purpose:

Implement gate-mediated action requests.

Includes:

- action request schema;
- grant checking;
- allowed/denied/approval-required outcomes;
- audit emission;
- deny tests.

Why third:

All effectful actions depend on gate().

### 4. `implement-telegram-owner-control-slice`

Purpose:

Build the first Lyra owner control workflow.

Includes:

- Telegram owner ID verification;
- `telegram.owner.message`;
- deterministic route to `main_assistant_agent`;
- owner-control task grant;
- status reply;
- setup/proposal stubs.

Why fourth:

It gives a usable interface without broad private-data access.

### 5. `implement-selected-thread-email-preview-slice`

Purpose:

Build the first guarded external communication workflow.

Includes:

- Gmail / Google Workspace selected-thread access;
- selection token;
- `email.thread.selected`;
- `email_reply_drafter`;
- model gateway wrapping;
- local preview;
- no final send.

Why fifth:

It proves private-data handling and prompt-injection boundaries after the core authority model exists.

## Trade-offs

| Option | Benefit | Cost |
|---|---|---|
| Use default `spec-driven` schema now | Fast, compatible with existing skills | Less tailored to security-heavy architecture |
| Create custom schema now | Better fit for research/decision-heavy work | Adds process work before implementation |
| Build Telegram first | Product feel from day one | Requires runtime skeleton first |
| Build schemas first | Safer foundation | Less visible product progress |
| Implement all PRD phases together | Faster apparent progress | High risk of unsafe, incoherent implementation |

Decision:

Use default `spec-driven` now. Define small implementation slices. Consider a custom schema later after two or three successful changes.

## Failure modes

| Failure mode | Response |
|---|---|
| Agent tries to implement whole PRD | Stop and create a smaller implementation slice |
| Proposal blurs OpenSpine and Lyra | Require layer classification |
| Change affects authority without marking it | Block review until marked and verification tasks added |
| Design reverses a decision without logging it | Add decision-log task |
| OpenSpec artifact treated as runtime authority | Reject; runtime authority must come from task grant/gate model |
| Tool-specific skills drift | Add drift-check task or canonical skill-generation task |

## Review checklist

Before applying any OpenSpine change:

- Does the proposal classify affected layer?
- Does it mark authority-sensitive areas?
- Does it include non-goals?
- Does the spec contain testable requirements?
- Does the design discuss authority impact if relevant?
- Does tasks.md include verification work?
- Has the decision log been checked?
- Is the change small enough to review?
