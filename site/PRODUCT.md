# OpenSpine site product context

## Product naming

Use the names at the level a reader needs:

- **OpenSpine is the self-hosted personal AI system a user installs.**
- **Lyra is the assistant included with OpenSpine and the one the user talks to.**
- **The OpenSpine runtime is the governed boundary underneath Lyra.**
- Builders may use that runtime for other governed assistant packages later.

Do not describe OpenSpine as an add-on that a finished third-party assistant plugs into. A general compatibility layer and native package resolver do not exist today.

## Core product promise

OpenSpine is designed for an employee-like personal AI whose capability and autonomy grow as the owner delegates more work.

The simple promise is:

> **Let your AI earn more responsibility, one job at a time.**

The supporting truth is:

> **Lyra can learn the job. It cannot promote itself.**

The product is not merely a hard limit around one task. The task boundary is the mechanism that makes progressive delegation possible without one model-controlled process accumulating open-ended authority.

## Product truth

OpenSpine is a self-hosted personal AI system. Lyra is its default owner-facing assistant package. Lyra composes agents, routes, workflows, capability packs, policies, templates, persona overlays, and memory scopes that the runtime loads and constrains.

For each task, the runtime verifies the request, composes bounded task permission, keeps connector and provider credentials outside contained workers, checks model-driven effects, and records the result. A small set of owner-selected pre-gate metadata reads is separately enumerated, classified, and audited.

OpenSpine also contains runtime support for governed growth:

- contained workers and structured results;
- declarative workflows and durable replay;
- versioned skills and promotion;
- standing rules with budgets, expiry, drift handling, pause, and revocation;
- reflection that can propose changes but cannot activate them;
- persona and preference artifacts kept separate from authority;
- versioned, diffable artifact lifecycles with provenance.

The intended owner experience is:

1. delegate one clear job;
2. let Lyra do internal work within that task boundary;
3. review the result and any real action or disclosure boundary;
4. let repeated approvals, corrections, and preferences produce a plain-language proposal;
5. approve, narrow, edit, reject, pause, expire, or revoke the reusable responsibility;
6. let future matching work require less interruption;
7. return drift, changed context, or exhausted budgets for review.

The complete owner-facing loop is not shipped yet. Runtime machinery has landed, but the natural product surface that joins it is tracked in [issue #123](https://github.com/George-RD/openspine/issues/123).

## Current alpha truth

The current owner-facing alpha is narrower than the product promise:

- direct local terminal conversation through `openspine chat`;
- verified Telegram owner control;
- one Gmail thread selected by the owner;
- reply drafting through a contained model path;
- exact-text and target approval before draft creation;
- email sending denied by runtime policy.

These are first-rung trust proofs. They are not the product ceiling and must not occupy the top of the message hierarchy as though OpenSpine were a Gmail drafting utility.

Do not present runtime-landed capability as a finished Lyra workflow. The roadmap must keep these states separate:

1. runtime landed;
2. wired into Lyra through an owner-facing path;
3. owner-facing product surface still missing.

## The user problem

People already understand the appeal of personal agents that can use tools, remember context, run workflows, and handle work.

Their unresolved problem is not simply “How do I make an agent safer?” It is:

> How does a useful assistant become more capable over time without quietly accumulating access to everything?

Most systems leave the owner between two poor choices:

- approve every meaningful action forever; or
- give a broad agent enough access to act independently from the start.

OpenSpine should explain a third path before teaching the ontology:

> Start with one bounded job. Review the result. Let repetition become a clear, revocable responsibility only after another decision.

Use concrete scenes:

- the first task can read one selected thread rather than the whole inbox;
- a poisoned email cannot grant access to more mail;
- a worker cannot expose a key it never received;
- a changed target no longer matches the task or approval;
- repeated approvals can produce a proposal rather than another prompt;
- the proposed routine stays inactive until reviewed;
- a budget, expiry, drift event, pause, or revocation ends the shortcut;
- preference learning cannot silently widen permission.

## Audience

### Primary reader: the interested personal-agent adopter

People who have seen OpenClaw, Hermes, or similar systems and want an assistant that becomes genuinely useful rather than remaining a chat interface.

They are attracted by capability, memory, tools, and autonomy. They hesitate when that capability depends on broad credentials or a permission surface that is difficult to inspect.

They do not begin with `runtime`, `authority`, `task grant`, `artifact lifecycle`, or `standing rule`. Start with the working relationship:

- delegate a job;
- review it;
- give more responsibility when the pattern is clear;
- retain the ability to limit, pause, and revoke it.

### Current adopter qualification: technical self-hoster

The alpha currently requires someone who can build the repository, configure a model provider, and optionally configure Telegram and Gmail OAuth.

This is an adoption constraint, not the language the hero should use to explain the offer. Do not confuse **who can install the alpha today** with **how a first-time reader understands why OpenSpine matters**.

### Secondary reader: agent product builder

Engineers and product-minded developers building agents that touch personal, company, or customer systems.

They need capability and autonomy that can grow without collapsing prompts, memory, policy, approvals, and credentials into one model-controlled process. They value declarative, versioned, testable responsibility and authority boundaries.

### Wrong fit today

- non-technical users expecting a polished consumer assistant;
- users choosing mainly on current channel and tool breadth;
- teams expecting production multi-tenant hosting;
- users expecting inbox-wide autonomy or automatic email sending now;
- users expecting a one-click installer;
- users expecting OpenSpine to run OpenClaw, Hermes, or arbitrary assistants today;
- users expecting the progressive delegation loop to be a finished owner experience already.

## Visitor scene

Most visitors arrive from GitHub, Hacker News, Reddit, self-hosted-agent communities, or discussion of capable personal agents.

They scan the first viewport for these answers:

1. Is this a personal AI system I can talk to?
2. Will it become more useful as I delegate more work?
3. How does more autonomy appear without broad access arriving up front?
4. Can the AI activate its own new routines or permissions?
5. What is working now, and what is still product direction?
6. Why should I believe the boundary is structural rather than a prompt promise?
7. What do I give up compared with a mature personal agent?

The page must answer the first four before leading with the narrow Gmail proof.

## Landing-page job

The page should:

1. name the desired outcome: a personal AI that grows into useful responsibility;
2. explain the growth model: delegate, review, propose, approve, repeat, revoke;
3. state the crucial guardrail: Lyra can learn the job but cannot promote itself;
4. establish that OpenSpine includes Lyra, the assistant the user talks to;
5. distinguish the product promise from current alpha breadth;
6. show the Gmail path as proof of the first task boundary, not as the whole offer;
7. explain the outside-the-model task and capability lifecycle;
8. state the honest trade-off against broader personal agents;
9. lead a qualified visitor into the working alpha, proof, roadmap, or source.

Primary action: **See how responsibility grows.**

Secondary action: **Run the alpha.**

## Messaging hierarchy

1. **Dream outcome:** let a personal AI earn more responsibility over time.
2. **Working relationship:** delegate one job, review it, then turn repetition into a reusable responsibility.
3. **Guardrail:** Lyra can learn the job; it cannot promote itself.
4. **Product shape:** OpenSpine system + Lyra assistant + governed runtime.
5. **Growth mechanism:** reviewable, versioned proposals; owner activation; budgets, expiry, pause, drift review, and revocation.
6. **Task mechanism:** short-lived task permission, contained workers, kernel-held credentials, and a gate before effects.
7. **Current alpha:** local chat and one guarded Gmail draft flow.
8. **Evidence:** named tests, deterministic composition, governed artifact lifecycle, credential separation, and audit.
9. **Competitive trade-off:** mature personal agents offer far more current breadth; OpenSpine prioritises governed growth.
10. **Honest limit:** the complete owner-facing progressive delegation loop is still product work.

## Available proof

### Task boundary

- One deterministic path: verify → identify → route → compose → grant → run → gate → audit.
- No matching allow means no grant; explicit deny wins.
- The contained worker receives no raw connector credentials.
- External content is wrapped as data and does not become permission.
- Selected targets and approvals are bound to the task and exact payload.
- Effectful worker actions pass through the gate before dispatch.
- Email sending is denied regardless of grant or approval state.

### Governed growth

- A proposed capability cannot widen authority before approval.
- Skills and other governed artifacts use versioned proposal, evaluation, approval, activation, retirement, and revocation paths.
- Standing rules are budgeted, expiring, revocable composition inputs; they do not replace or widen the task grant.
- Repeated saturation or drift can move a rule back to review rather than silently widening it.
- Reflection can propose correction, preference, rule, and consolidation artifacts without direct state mutation or activation.
- Personality, memory, authority, and task prompts remain separate concerns.

### Claim discipline

- Documented public security claims map to named tests.
- The build checks that those tests continue to exist.
- Owner-facing availability requires an end-to-end owner path; runtime existence alone is insufficient.

## Competitive posture

OpenClaw and Hermes currently provide far more channels, tools, skills, automation, and onboarding. Do not claim feature parity or say they have no security model.

The fair user-facing distinction is:

> **Capability should grow through delegation, not arrive as open-ended access.**

The useful architecture distinction is:

> Mature personal agents optimise for broad capability and add controls around it. OpenSpine starts with bounded tasks and gives reusable responsibility a separate reviewed lifecycle.

Use named mechanisms when comparing. Do not use competitor names as an attack headline.

## Voice

Direct, plain, concrete, and falsifiable.

Lead with the desired working relationship, then explain the growth mechanism. Use the current Gmail path only after the reader understands that it is the first rung.

A reader should be able to explain the offer without repeating an architecture term:

> “I delegate one job. If the work repeats, Lyra can suggest a reusable responsibility. I decide whether it becomes active and can stop it later.”

Prefer:

- “Let your AI earn more responsibility. One job at a time.”
- “Lyra can learn the job. It cannot promote itself.”
- “Delegate, review, then repeat.”
- “Turn repeated work into a reviewable responsibility.”
- “You choose what becomes reusable, where it applies, and when it expires.”
- “Capability grows. Access does not drift.”
- “The worker never received the key.”
- “OpenClaw and Hermes offer far more features today.”
- “Run the named test.”

Avoid:

- leading with the single Gmail draft as the product promise;
- describing OpenSpine primarily as a safer email tool;
- implying the progressive delegation owner experience is already complete;
- starting the hero with `runtime`, `authority`, `scope`, `task grant`, `standing rule`, or `model-driven worker`;
- “A permission layer for AI assistants” as the main category;
- “Use your AI assistant with OpenSpine” until a compatibility interface exists;
- “Secure AI” without a named mechanism;
- “Military-grade,” “enterprise-ready,” “unhackable,” or universal safety claims;
- invented users, adoption, benchmarks, customer logos, or production-readiness claims;
- treating an LLM, robot, or glowing brain as the product.

## Visual direction

Show progression and governed responsibility, not only denial:

- one bounded task entering the system;
- review and receipt;
- repetition becoming a proposed routine;
- an owner decision activating a limited, expiring responsibility;
- a changed target, budget limit, pause, or revocation returning control;
- current alpha proof clearly labelled as the first rung.

Visual anti-references:

- black-and-neon cybersecurity landing pages;
- purple-to-blue AI gradients and glowing borders;
- literal bones, vertebrae, or medical imagery;
- stock robots, humanoid assistants, and abstract neural networks;
- equal icon cards used as the whole page structure;
- fake dashboards or metrics that imply usage the project has not earned.

## Decision records

- [`.raw/openspine-progressive-delegation-positioning-2026-07-31.md`](../.raw/openspine-progressive-delegation-positioning-2026-07-31.md) records the correction from narrow proof to the employee-like progressive delegation promise.
- [`.raw/openspine-layperson-offer-rerun-2026-07-30.md`](../.raw/openspine-layperson-offer-rerun-2026-07-30.md) records the earlier buyer and trust-gap rerun.
- [Issue #123](https://github.com/George-RD/openspine/issues/123) owns the missing owner-facing delegation-growth loop.
