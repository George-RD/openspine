# OpenSpine positioning audit

Date: 2026-07-30
Status: Product-positioning decision record for the website, README, and roadmap
Method: Growth Arsenal Grand Slam Offer phases, paired-copy evaluation, competitor primary sources, and current repository truth

## Executive finding

The previous line, **“a self-hosted permission layer for AI assistants,”** described an important part of OpenSpine but misclassified the product.

It implied that a user already had an assistant and added OpenSpine underneath it. That is not the current user experience. A user installs OpenSpine, talks to Lyra, and runs workflows through the OpenSpine runtime. Third-party assistants do not plug into OpenSpine through a finished compatibility interface today.

The clearer layered model is:

1. **OpenSpine is the self-hosted personal AI system a user installs.**
2. **Lyra is the default assistant package the user talks to.**
3. **The OpenSpine runtime is the trust boundary underneath Lyra.**
4. **Builders can use that runtime for other governed agent packages later.**

This does not reverse D-023. It gives the user-facing system, the default assistant, and the reusable runtime separate names at the level where each matters.

## The market problem

OpenClaw and Hermes make it easy to give an assistant more channels, tools, skills, memory, terminal access, and automation. That is valuable. The trust problem appears when a capable assistant stops being a chat interface and starts touching an inbox, customer data, files, infrastructure, or another account.

The user is then asked to accept one or more of these trade-offs:

- broad credentials remain available to an agent-controlled process;
- prompt rules and model judgement carry part of the safety burden;
- a malicious email, page, tool result, or skill can influence the same model that chooses actions;
- approvals focus on commands or individual tools instead of a complete task boundary;
- permissions can be scattered across channel, tool, sandbox, plugin, and prompt configuration;
- the user has to choose between useful autonomy and constant confirmation.

OpenClaw's own security guide says its trusted single-operator default may allow host execution without approval and that sender-scoped controls do not sanitize quoted, fetched, attachment, tool-result, or other prompt content. Its issue tracker contains requests for masked secrets, scope-bound credentials, stronger plugin isolation, and security enforcement outside the LLM.

Hermes provides approvals, deny rules, container backends, prompt scanning, and credential filtering. Its default smart command approval uses an auxiliary LLM, the agent can create and modify skills, and its documentation describes deny rules as guardrails for an honest-but-wrong agent rather than a sandbox against an adversarial process.

These projects are not presented as careless. They optimize for broad assistant capability and then add configurable safety layers. OpenSpine starts from a different premise: **the model may be wrong or manipulated, so it must never own the authority boundary.**

Primary sources:

- OpenClaw README: https://github.com/openclaw/openclaw/blob/main/README.md
- OpenClaw security model: https://github.com/openclaw/openclaw/blob/main/docs/gateway/security/index.md
- OpenClaw masked-secrets request: https://github.com/openclaw/openclaw/issues/10659
- OpenClaw security-profile request: https://github.com/openclaw/openclaw/issues/8719
- OpenClaw plugin/sandbox request: https://github.com/openclaw/openclaw/issues/12505
- Hermes README: https://github.com/NousResearch/hermes-agent/blob/main/README.md
- Hermes security model: https://github.com/NousResearch/hermes-agent/blob/main/website/docs/user-guide/security.md
- Hermes skills model: https://github.com/NousResearch/hermes-agent/blob/main/website/docs/user-guide/features/skills.md

## Phase 0: product and audience

### Product

OpenSpine is a self-hosted personal AI system with a governed runtime at its core. It ships with Lyra as the default owner-facing assistant package.

Lyra coordinates agents and workflows. The runtime verifies requests, keeps credentials outside workers, builds a short-lived task grant, gates model-driven effects before dispatch, records the result, and controls how capability can grow. A small set of owner-selected pre-gate metadata reads is separately enumerated, classified, and audited.

### Primary audience

**Technical self-hosters and agent power users who want a personal assistant to do real work, but stop short of connecting sensitive accounts because the blast radius is too large or too hard to reason about.**

They are not looking for another chat UI. They already understand the appeal of a capable agent. Their unresolved problem is trust.

### Secondary audience

**Agent builders who need a reusable runtime boundary for products that touch customer, company, or personal systems.**

They need permissions that remain true when prompts change, models are swapped, skills are added, or external content is malicious.

### Wrong-fit audience today

- non-technical users expecting a polished consumer assistant;
- users who mainly want the widest tool and channel catalogue now;
- teams expecting production multi-tenant hosting;
- users expecting inbox-wide autonomy, automatic email sending, or a one-click installer.

Rejecting the wrong fit is important. The current alpha is a trust proof with a working assistant path, not a feature-complete OpenClaw or Hermes replacement.

## Phase 1: starving-crowd assessment

| Indicator | Score | Evidence and limitation |
|---|---:|---|
| Pain | 9/10 | The user wants the value of real account access but treats credential leakage, prompt injection, wrong-target actions, and silent permission growth as unacceptable. |
| Purchasing power | 7/10 | The audience already spends money or time on models, VPSs, hardware, and self-hosting. OpenSpine is open source, so the relevant purchase is setup time and trust, not a current price. |
| Easy to target | 8/10 | They gather around self-hosted agent projects, security discussions, local-AI communities, GitHub, Hacker News, and technical forums. |
| Growth | 8/10 | Personal agent platforms are adding more tools, channels, memory, skills, and automation. Every added capability increases the need for a clearer authority boundary. |

**Assessment: 32/40.** The niche is strong, but only if the message targets the trust gap. “Permission layer” sounds like infrastructure sold to someone who already decided to build an agent. “Personal AI system for real account access” connects the mechanism to the user's stalled outcome.

## Customer personas

### 1. The cautious agent power user

- Already runs or has tried OpenClaw, Hermes, or a similar system.
- Likes the capability but will not expose a main inbox, customer data, infrastructure, or broad filesystem access.
- Current workaround: keep the agent in chat, use a disposable environment, approve everything, or do sensitive work manually.
- Objection: “This looks safer, but it does much less and takes more work to install.”
- Dream outcome: “I want the assistant to handle real work without giving it a route to everything else.”

### 2. The agent product builder

- Building an assistant or workflow that touches real systems.
- Current workaround: prompts, tool allowlists, sandbox settings, approval hooks, and custom logging spread across the application.
- Objection: “Why adopt a new runtime instead of hardening my existing stack?”
- Dream outcome: “I want one testable place that decides what every task may do, regardless of the model or prompt.”

### 3. The capable-agent sceptic

- Has seen an agent take an unexpected action, expose a secret, target the wrong object, or follow poisoned content.
- Current workaround: no autonomy.
- Objection: “Every agent project says it is safe.”
- Dream outcome: “Show me the exact limit, the exact approval, and the test that proves the limit is enforced.”

## Phase 3: value equation

| Variable | Current score | What raises it | What lowers it |
|---|---:|---|---|
| Dream outcome | 9/10 | A genuinely useful personal assistant that can touch real systems without open-ended authority. | The current Gmail slice is much narrower than the full vision. |
| Perceived likelihood | 5/10 | Runtime-enforced grants, contained workers, named tests, digest-bound approval, and explicit denials. | Alpha status, little external adoption evidence, and no broad workflow demonstration. |
| Time delay | 3/10 | The kernel and Lyra already run end to end. | Docker, Telegram, model, Gmail OAuth, mailbox configuration, and copied thread IDs delay the first win. |
| Effort and sacrifice | 3/10 | Self-hosted and inspectable, with a documented path. | The user gives up the convenience, breadth, and onboarding quality of mature assistant systems. |

### Value-equation conclusion

The architecture is not the main messaging failure. The offer is currently weak on **speed and effort**. Copy can make the reason to care obvious, but copy cannot compensate for setup friction and a single narrow workflow.

The website should therefore:

- sell the dream outcome first;
- explain the unique mechanism second;
- show the current alpha honestly;
- avoid implying feature parity with mature assistants;
- point the roadmap toward a faster first useful task.

## Phase 4: problem-to-solution stack

| User problem | OpenSpine mechanism | Plain-English result |
|---|---|---|
| “The agent can see my keys.” | Credentials stay in the kernel; contained workers receive a task token, not raw connector secrets. | A bad model response cannot print or reuse a key it never received. |
| “A poisoned email can tell the agent what to do.” | External content is wrapped as data and cannot create authority. | The email may influence the draft, but it cannot grant access to more email or a new action. |
| “The agent might act on the wrong customer, thread, or account.” | Identity and scope-bearing parameters are bound by the kernel and the task grant. | The worker cannot quietly switch the target. |
| “Approving one action gives it a permanent habit.” | Approval is digest-bound; authority growth is a separate versioned proposal. | The exact thing reviewed is approved. More access needs another explicit decision. |
| “I cannot tell which rule actually won.” | Routes, agents, workflows, packs, policy, and caveats resolve into one task grant; explicit deny wins. | Each task has one inspectable permission result. |
| “Safety claims are just marketing.” | Claims map to named tests and the build checks the mapping. | A claim is tied to something the user can run and falsify. |
| “An assistant that asks every time is not useful.” | Standing rules, one-loop plan approval, quiet internal work, and owner digests are the intended autonomy model. | Repeated safe work becomes smoother without moving authority into the prompt. |

### Core offer

**A self-hosted personal AI system for real account access, with authority kept outside the model.**

### What ships in the offer today

- the OpenSpine governed runtime;
- Lyra, the default assistant package;
- verified Telegram owner control;
- a selected-thread Gmail drafting workflow;
- exact-text approval before draft creation;
- hard-denied email sending;
- contained workers, task grants, a gate for worker-requested effects, explicit audited pre-gate paths, encrypted artifacts, audit receipts, and test-backed claims.

### What belongs to the product direction, not the current promise

- one-command package installation;
- a polished thread picker;
- calendar, files, infrastructure, CRM, and customer-service workflows;
- turnkey third-party assistant compatibility;
- consumer-grade onboarding;
- broad autonomous execution without setup.

## Competitive position

Do not claim that OpenSpine is simply “more secure” than OpenClaw or Hermes. Both projects provide serious security controls, and they currently provide far more user-facing capability.

Use this distinction instead:

| Product emphasis | OpenClaw / Hermes | OpenSpine |
|---|---|---|
| Primary product story | A capable personal agent with many tools, channels, skills, and automations | A personal AI system designed around enforceable task boundaries |
| Safety posture | Configurable controls, approvals, sandboxes, allowlists, and security guidance around a broad agent | Authority composed outside the model; workers receive only the task grant; model-driven effects cross one gate before dispatch; trusted pre-gate reads are explicit and audited |
| Current strength | Capability breadth, onboarding, channels, mature user experience | Structural limits, auditable authority, test-backed security claims |
| Current weakness | Broad capability creates a larger and more complex trust surface | Narrow alpha, high setup effort, few end-user workflows |

The useful sentence is:

> OpenClaw and Hermes are capability-first assistants. OpenSpine is a trust-first assistant system.

Use it in a comparison document, not as an attack headline.

## Messaging hierarchy

A first-time reader should learn these points in order:

1. **What it is:** a self-hosted personal AI system.
2. **Why it exists:** normal agents become hard to trust when they receive real account access.
3. **What the user gets:** an assistant that can do bounded work without holding the master key.
4. **How it differs:** the model does not decide its own permissions; the runtime does.
5. **What works today:** Lyra can draft from one selected Gmail thread and create the exact approved draft; sending is blocked.
6. **Why believe it:** named tests, contained workers, grants, gate, explicit trusted paths, and audit.
7. **What to do:** inspect the working boundary, then run the alpha.

## Recommended copy direction

### Category line

**Self-hosted personal AI with hard limits.**

### Headline

**Give your AI real work.\nNot the master key.**

### Supporting copy

OpenSpine is the system you install. Lyra is the assistant you talk to. The runtime keeps your account keys away from the model. Each task gets a short-lived scope. The model-driven worker cannot reach your accounts beyond that scope.

### Current-proof line

Today, Lyra can read one Gmail thread you select, draft a reply, and create the exact draft you approved. It cannot send the email.

### Primary call to action

**See the working boundary**

A visitor should understand the proof before being asked to invest in the setup. “Run the alpha” remains the next action for a qualified technical reader.

## Copy rules

- Say **OpenSpine is the system** and **Lyra is the default assistant**.
- Say **runtime** only after the user understands the product.
- Translate `authority` to “what the task is allowed to do” on first use.
- Translate `task grant` to “short-lived task permissions” on first use.
- Use real failure scenes: wrong thread, poisoned email, leaked key, changed draft, silent new capability.
- Say the gate applies to model-driven or worker-requested effects; name the small enumerated pre-gate owner-selected paths when technical precision matters.
- Do not imply that agents can currently propose skills through the public Lyra path. Skills install through a verified-owner command and a separate promotion lifecycle today.
- Do not use “safe,” “secure,” “trustworthy,” or “cannot” without naming the enforced mechanism or bounded claim.
- Keep OpenClaw and Hermes comparisons factual and acknowledge their capability advantage.
- Separate **current alpha** from **north-star system** in every high-level surface.

## Architecture and roadmap contradictions

### 1. The previous copy implied bring-your-own assistant support

“A permission layer for AI assistants” sounded like middleware. The current system instead loads Lyra as its default package. The native package installer and a general third-party integration surface are not shipped.

**Action:** change the user-facing category. Keep “runtime for governed agents” in architecture material. Addressed in this change.

### 2. The product vision is ahead of the default user experience

The design canon describes a chief-of-staff system with quiet internal work, standing rules, workers, memory, reflection, and receipts. The public alpha still asks the user to copy a Gmail thread ID into Telegram.

**Action:** treat the current Gmail path as proof of the boundary, not the whole offer. Prioritize a first useful task that feels like an assistant. Raised as issue #118.

### 3. The planned package interface was not on the visible roadmap

`docs/lyra.md` states the intended flow:

```text
openspine install lyra
openspine use lyra
openspine run
```

The productize-Lyra change explicitly defers the transactional package store and resolver. The previous public roadmap did not make that follow-up visible.

**Action:** add an explicit product roadmap item and issue for the installer and selected-package model. Raised as issue #117 and linked from the revised roadmap.

### 4. Richer onboarding is deferred by architecture sequence, but onboarding is now the main value leak

The day-two operations brief defers richer onboarding until a second deployment exists. The Grand Slam value equation shows that setup time and effort are the weakest parts of the offer now.

**Action:** do not weaken the runtime sequence, but create a separate product-surface track for install, setup validation, first-run progress, and recovery. Product UX work should not wait for a second deployment target. Raised as issue #118.

### 5. The public roadmap had factual drift

The previous public roadmap listed secret intake as deferred, while `implement-secret-intake` is archived in the change sequence. It also understated the agent-OS work that has landed.

**Action:** update the roadmap from the change ledger and separate shipped kernel capability from user-visible product capability. Addressed in this change.

### 6. Advanced kernel capability is not the same as an end-user feature

The change ledger records standing rules, workers, skills, task boards, reflection, disclosure policy, and other machinery as archived. The website should not imply that each is available through a complete Lyra workflow unless it has been exercised end to end from the user surface.

**Action:** label capabilities as `runtime`, `wired into Lyra`, or `planned product surface`. Raised as issue #119 and reflected in the revised roadmap.

### 7. Universal single-gate language overstated the current trusted-path model

The Gmail workflow includes a narrowly enumerated owner-selected metadata read before grant composition. It is classified as `PreGateOwnerSelectedRead`, not a worker effect mediated by `gate()`.

**Action:** bound public claims to model-driven worker effects and document the explicit, audited trusted-path carve-out. Addressed across the README, comparison, Why page, architecture page, product context, landing page, and boundary visual.

### 8. Agent-proposed skill language was ahead of the shipped Lyra path

Skills exist as governed artifacts, but the public path installs them through a verified-owner command. The agent-facing artifact proposal allowlist does not currently accept skills.

**Action:** remove skills from current agent-proposal claims and label agent-proposed skill installation as future product work. Addressed in the README and Why page.

## Decision

Adopt a **trust-first personal AI system** position.

Use this product model consistently:

```text
OpenSpine system
├── OpenSpine runtime      # verifies, grants, gates, records
└── Lyra package           # the default assistant and workflows
```

This makes the product understandable without hiding the architecture. The assistant is what the user gets. The runtime boundary is why they should believe it can eventually be trusted with more.