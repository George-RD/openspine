# OpenSpine site product context

## Product truth

OpenSpine is a self-hostable runtime substrate for governed agents. It keeps authority outside the language model and system prompt. Each event is verified, routed, resolved into a bounded task grant, passed through a single effect gate, and recorded in a tamper-evident audit trail.

Lyra is the first product built on OpenSpine. It is a Telegram-controlled personal assistant whose first guarded workflow reads only a Gmail thread the owner selected and prepares a reply. Email sending is denied by runtime policy.

The project is alpha. The substrate and Lyra run end to end, while the roadmap names work that has not landed.

## Audience

### Primary: technical self-hosters

People who want an assistant to help with real email, messages, and life admin, but do not accept blanket credentials plus a prompt that says “be careful.” They can run software, inspect a repository, and follow a technical quickstart. They need a concrete reason to trust the boundary before investing setup time.

### Secondary: agent product builders

Engineers and product-minded developers building agents that touch customer, company, or personal systems. They are tired of treating safety as prompt wording. They want deterministic authority, explicit approval, narrow scope, and tests they can run.

## Visitor scene

Most visitors arrive from GitHub, Hacker News, Reddit, or a technical discussion. They are curious but skeptical. They scan the first viewport for three answers:

1. What does this do?
2. How is it materially different from another agent framework?
3. Where is the proof?

The page must answer those before explaining the full ontology.

## Landing-page job

The page should make the offer intelligible, demonstrate the unique mechanism, and lead a qualified visitor into the quickstart or threat model.

Primary action: **Run the quickstart.**

Secondary action: **Inspect the threat model and named tests.**

## Available proof

- One deterministic runtime path: verify → identify → route → compose → grant → run → gate → audit.
- Authority is the intersection of route, agent, workflow, capability, and policy constraints.
- No matching allow means no grant; explicit deny wins.
- The agent shell receives no raw connector credentials.
- External content is wrapped as data, never accepted as authority.
- Selected-thread access is bound to a single-use selection token.
- `email.send` is denied regardless of grant or approval state.
- Documented security claims map to named tests, and the build checks that those tests continue to exist.

## Voice

Direct, plain, technical, and falsifiable. Lead with the user problem and the runtime result. Use architecture terms only where they add precision. State alpha limits without apology or hype.

Prefer:

- “The runtime decides.”
- “The model reasons; the spine decides.”
- “Email sending is denied by policy.”
- “Run the named test.”

Avoid:

- “Secure AI” without a named mechanism.
- “Military-grade,” “enterprise-ready,” “unhackable,” or universal safety claims.
- Invented users, adoption, benchmarks, customer logos, or production-readiness claims.
- Treating an LLM, robot, or glowing brain as the product.

## Visual anti-references

- Black-and-neon cybersecurity landing pages.
- Purple-to-blue AI gradients and glowing borders.
- Literal bones, vertebrae, or medical imagery.
- Stock robots, humanoid assistants, and abstract neural networks.
- Equal icon cards used as the whole page structure.
- Fake dashboards or metrics that imply usage the project has not earned.
