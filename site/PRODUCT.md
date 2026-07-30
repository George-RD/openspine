# OpenSpine site product context

## Product naming

Use the names at the level a reader needs:

- **OpenSpine is the self-hosted personal AI system a user installs.**
- **Lyra is the default assistant package the user talks to.**
- **The OpenSpine runtime is the governed boundary underneath Lyra.**
- Builders may use that runtime for other agent packages later.

Do not describe OpenSpine as an add-on that a finished third-party assistant plugs into. A general compatibility layer and native package resolver do not exist today.

## Product truth

OpenSpine is a self-hosted personal AI system designed for the point where an assistant starts touching real accounts and data. The runtime keeps authority outside the language model and system prompt. Each event is verified, routed, resolved into bounded task permissions, passed through one effect gate, and recorded in a tamper-evident audit trail.

Lyra is the default owner-facing assistant package. It composes agents, routes, workflows, capability packs, policies, templates, persona overlays, and memory scopes that the runtime loads and constrains.

The project is alpha. The current user-visible proof is narrow: through a verified Telegram channel, Lyra can read one Gmail thread the owner selected, draft a reply, and create the exact draft the owner approved. Email sending is denied by runtime policy.

The north star is broader: a chief-of-staff-style personal AI that can gain useful capability without gaining open-ended authority. Do not present that north star as shipped product breadth.

## The user problem

Capability is easy to demonstrate. Trust becomes hard when an agent receives an inbox token, filesystem access, customer data, infrastructure controls, or another real credential.

The target user already understands why a capable agent is useful. Their unresolved question is:

> How do I let it do real work without giving the model a route to everything else?

Connect every architecture term to that problem. Use concrete failure scenes: a poisoned email, the wrong thread, an exposed key, a changed draft, a skill that adds an extra recipient, or an agent that quietly asks for broader access.

## Audience

### Primary: technical self-hosters and agent power users

People who run or are considering systems such as OpenClaw or Hermes, but stop before connecting sensitive accounts because the blast radius is too large or difficult to inspect. They can run software, inspect a repository, and follow a technical quickstart. They need a concrete reason to accept OpenSpine's narrower alpha and higher setup effort.

### Secondary: agent product builders

Engineers and product-minded developers building agents that touch customer, company, or personal systems. They want authority that remains enforceable when prompts change, models are swapped, skills are added, or external content is hostile.

### Wrong fit today

- non-technical users expecting a polished consumer assistant;
- users choosing mainly on channel and tool breadth;
- teams expecting production multi-tenant hosting;
- users expecting inbox-wide autonomy or automatic email sending;
- users expecting a one-click installer.

## Visitor scene

Most visitors arrive from GitHub, Hacker News, Reddit, a self-hosted-agent community, or a technical discussion. They are interested in personal agents but sceptical of broad access. They scan the first viewport for five answers:

1. Is OpenSpine the assistant system, or middleware for another assistant?
2. What real problem does it solve for me?
3. How is it materially different from a capable agent with prompts, approvals, and a sandbox?
4. What works today?
5. Where is the proof?

The page must answer those before explaining the full ontology.

## Landing-page job

The page should:

1. establish OpenSpine as a self-hosted personal AI system;
2. name Lyra as the assistant the user talks to;
3. connect the trust problem to real account access;
4. explain the outside-the-model authority boundary;
5. show the current Gmail workflow as proof, not as the whole product;
6. lead a qualified visitor into the working boundary, quickstart, or threat model.

Primary action: **See the working boundary.**

Secondary action: **Run the alpha.**

## Messaging hierarchy

1. **Outcome:** give a personal AI real work without giving the model the master key.
2. **Category:** self-hosted personal AI with hard limits.
3. **Product shape:** OpenSpine system + Lyra assistant + governed runtime.
4. **Unique mechanism:** task-specific authority is composed outside the model and every effect crosses one gate.
5. **Current proof:** one selected Gmail thread, exact-text approval, draft creation, send denied.
6. **Evidence:** named tests, contained workers, credential separation, audit chain.
7. **Honest limit:** alpha setup is technical and workflow breadth is narrow.

## Available proof

- One deterministic runtime path: verify → identify → route → compose → grant → run → gate → audit.
- Authority is the intersection of route, agent, workflow, capability, policy, caveat, approval, and runtime limits.
- No matching allow means no grant; explicit deny wins.
- The contained worker receives no raw connector credentials.
- External content is wrapped as data and does not become authority.
- Selected-thread access is bound to a single-use selection token.
- Approval is digest-bound to the exact payload and target reviewed by the owner.
- `email.send` is denied regardless of grant or approval state.
- Documented security claims map to named tests, and the build checks that those tests continue to exist.

## Competitive posture

OpenClaw and Hermes currently provide far more channels, tools, skills, automation, and onboarding. Do not claim feature parity or say they have no security model.

The fair distinction is:

> OpenClaw and Hermes are capability-first assistants. OpenSpine is a trust-first assistant system.

Use named mechanisms when comparing. Do not use competitor names as an attack headline.

## Voice

Direct, plain, technical, and falsifiable. Lead with the user's stalled outcome, then the runtime result. Translate necessary terms once. State alpha limits without apology or hype.

Prefer:

- “OpenSpine is the system. Lyra is the assistant you talk to.”
- “Give your AI real work. Not the master key.”
- “The model can ask. The runtime decides.”
- “One task gets one short-lived set of permissions.”
- “The worker never received the key.”
- “Email sending is denied by policy.”
- “Run the named test.”

Avoid:

- “A permission layer for AI assistants” as the main category.
- “Use your AI assistant with OpenSpine” until a compatibility interface exists.
- “Secure AI” without a named mechanism.
- “Military-grade,” “enterprise-ready,” “unhackable,” or universal safety claims.
- Invented users, adoption, benchmarks, customer logos, or production-readiness claims.
- Treating the north-star chief-of-staff experience as already shipped.
- Treating an LLM, robot, or glowing brain as the product.

## Visual anti-references

- Black-and-neon cybersecurity landing pages.
- Purple-to-blue AI gradients and glowing borders.
- Literal bones, vertebrae, or medical imagery.
- Stock robots, humanoid assistants, and abstract neural networks.
- Equal icon cards used as the whole page structure.
- Fake dashboards or metrics that imply usage the project has not earned.