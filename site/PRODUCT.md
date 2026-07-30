# OpenSpine site product context

## Product naming

Use the names at the level a reader needs:

- **OpenSpine is the self-hosted personal AI system a user installs.**
- **Lyra is the assistant included with OpenSpine and the one the user talks to.**
- **The OpenSpine runtime is the governed boundary underneath Lyra.**
- Builders may use that runtime for other agent packages later.

Do not describe OpenSpine as an add-on that a finished third-party assistant plugs into. A general compatibility layer and native package resolver do not exist today.

## Product truth

OpenSpine is a self-hosted personal AI system designed for the point where an assistant starts touching real accounts and data. The runtime keeps permission decisions outside the language model and system prompt. Each event is verified, routed, and resolved into bounded task permissions. Model-driven effects pass through one gate before dispatch. A small set of owner-selected pre-gate metadata reads is separately enumerated, classified, and audited.

Lyra is the default owner-facing assistant package. It composes agents, routes, workflows, capability packs, policies, templates, persona overlays, and memory scopes that the runtime loads and constrains.

The project is alpha. The current user-visible proof is narrow: through a verified Telegram channel, Lyra can read one Gmail thread the owner selected, draft a reply, and create the exact draft the owner approved. Email sending is denied by runtime policy.

The north star is broader: a chief-of-staff-style personal AI that can gain useful capability without gaining open-ended access. Do not present that north star as shipped product breadth.

## The user problem

People already understand the appeal of a personal AI that can use tools, remember context, and handle work. The hesitation arrives when setup asks for a main inbox, files, customer data, infrastructure controls, or another real account.

The first question is not about runtimes, authority, or task grants. It is:

> Can I let this help without giving it access to everything?

OpenSpine should answer that question before explaining the architecture.

Use concrete scenes:

- one selected email thread rather than the whole inbox;
- a poisoned email that cannot grant access to more mail;
- a worker that cannot expose a key it never received;
- an attempted action on the wrong thread, customer, or account;
- a changed draft or extra recipient that no longer matches the approval;
- an assistant that cannot quietly give itself broader access.

## Audience

### Primary reader: the interested but cautious personal-agent user

People who have seen the promise of OpenClaw, Hermes, or similar personal agents and want the practical result. They pause at broad access to sensitive accounts because the possible reach feels too large or too hard to inspect.

They may understand AI products without knowing security architecture. Start in their language: the job, the account, what stays out of reach, and what happens if the assistant tries something else.

### Current adopter qualification: technical self-hoster

The alpha currently requires someone who can use Docker, configure Telegram, connect a model provider and Gmail OAuth, and follow a repository quickstart. This is an adoption constraint, not the opening language of the offer.

Do not confuse **who can install the alpha today** with **how a first-time reader understands why it matters**.

### Secondary reader: agent product builder

Engineers and product-minded developers building agents that touch customer, company, or personal systems. They want permissions that remain enforceable when prompts change, models are swapped, skills are added, or external content is hostile.

### Wrong fit today

- non-technical users expecting a polished consumer assistant;
- users choosing mainly on channel and tool breadth;
- teams expecting production multi-tenant hosting;
- users expecting inbox-wide autonomy or automatic email sending;
- users expecting a one-click installer;
- users expecting OpenSpine to run OpenClaw, Hermes, or arbitrary assistants today.

## Visitor scene

Most visitors arrive from GitHub, Hacker News, Reddit, a self-hosted-agent community, or discussion of personal-agent products. They are interested in what capable agents can do and sceptical of the access those agents may require.

They scan the first viewport for six answers:

1. Is this a personal AI I can talk to, or infrastructure for developers?
2. Does it address the reason I have not connected my real accounts?
3. What can it do today?
4. What exactly stays out of reach?
5. Why believe that limit is enforced?
6. What do I give up compared with a mature agent?

The page must answer those before teaching the full ontology.

## Landing-page job

The page should:

1. name the desired result: let a personal AI do useful work;
2. name the hesitation: broad access to inboxes, files, and accounts;
3. give the plain-English result: one task can use only what that task needs;
4. establish that OpenSpine includes Lyra, the assistant the user talks to;
5. show the current Gmail workflow as proof, not as the whole product;
6. state the honest trade-off against broader personal agents;
7. explain the outside-the-model permission boundary;
8. lead a qualified visitor into the working proof, quickstart, or threat model.

Primary action: **See the limits in action.**

Secondary action: **Run the alpha.**

## Messaging hierarchy

1. **Desire:** let a personal AI do the job.
2. **Hesitation:** keep the rest of the user's accounts out of reach.
3. **Plain result:** the user chooses the task; Lyra can use only what that task needs.
4. **Current proof:** one selected Gmail thread, exact-text approval, draft creation, sending denied.
5. **Product shape:** OpenSpine system + Lyra assistant + governed runtime.
6. **Competitive trade-off:** mature personal agents offer more breadth; OpenSpine puts hard task limits first.
7. **Unique mechanism:** task permission is composed outside the model; model-driven effects cross one gate before dispatch; trusted pre-gate paths are explicit and audited.
8. **Evidence:** named tests, contained workers, credential separation, and an audit chain.
9. **Honest limit:** alpha setup is technical and workflow breadth is narrow.

## Available proof

- One deterministic runtime path: verify → identify → route → compose → grant → run → gate → audit.
- Permission is the intersection of route, agent, workflow, capability, policy, caveat, approval, and runtime limits.
- No matching allow means no grant; explicit deny wins.
- The contained worker receives no raw connector credentials.
- External content is wrapped as data and does not become permission.
- Selected-thread access is bound to a single-use selection token.
- Effectful worker actions pass through the gate before dispatch.
- A small set of owner-selected pre-gate metadata reads is separately classified and audited.
- Approval is digest-bound to the exact payload and target reviewed by the owner.
- `email.send` is denied regardless of grant or approval state.
- Documented security claims map to named tests, and the build checks that those tests continue to exist.

## Competitive posture

OpenClaw and Hermes currently provide far more channels, tools, skills, automation, and onboarding. Do not claim feature parity or say they have no security model.

The fair user-facing distinction is:

> OpenClaw and Hermes offer far more features today. OpenSpine makes a different trade: each task gets hard limits before the AI can reach your accounts.

The useful architecture distinction is:

> Mature personal agents optimise for broad capability and add controls around it. OpenSpine starts with the task boundary and grows capability inside it.

Use named mechanisms when comparing. Do not use competitor names as an attack headline.

## Voice

Direct, plain, concrete, and falsifiable. Lead with the user's desired job and hesitation. Show one bounded example. Then explain the runtime result.

A reader should be able to state the desire and hesitation without repeating an architecture term. If they cannot, plain wording has not yet produced plain understanding.

Prefer:

- “Let a personal AI do the job. Keep the rest of your accounts out of reach.”
- “OpenSpine comes with Lyra, the assistant you talk to.”
- “You choose the task. Lyra can use only what that task needs.”
- “Choose one Gmail thread. Lyra cannot quietly switch to another.”
- “The worker never received the key.”
- “Anything outside the task is blocked or brought back to you.”
- “OpenClaw and Hermes offer far more features today.”
- “Run the named test.”

Avoid:

- starting the hero with `runtime`, `authority`, `scope`, `task grant`, or `model-driven worker`;
- “A permission layer for AI assistants” as the main category;
- “Use your AI assistant with OpenSpine” until a compatibility interface exists;
- “Secure AI” without a named mechanism;
- “Military-grade,” “enterprise-ready,” “unhackable,” or universal safety claims;
- invented users, adoption, benchmarks, customer logos, or production-readiness claims;
- treating the north-star chief-of-staff experience as already shipped;
- treating an LLM, robot, or glowing brain as the product.

## Visual anti-references

- Black-and-neon cybersecurity landing pages.
- Purple-to-blue AI gradients and glowing borders.
- Literal bones, vertebrae, or medical imagery.
- Stock robots, humanoid assistants, and abstract neural networks.
- Equal icon cards used as the whole page structure.
- Fake dashboards or metrics that imply usage the project has not earned.

## Decision record

The detailed rerun and paired evaluation live in [`.raw/openspine-layperson-offer-rerun-2026-07-30.md`](../.raw/openspine-layperson-offer-rerun-2026-07-30.md).
