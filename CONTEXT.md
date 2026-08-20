# OpenSpine — domain glossary

Terms here are canonical: cite promises and users by name. This is a glossary
only. Failure modes live in [STORIES.md](STORIES.md); standing rulings and the
normative decision test live in [DIRECTION.md](DIRECTION.md).

## What OpenSpine is

**OpenSpine**: a governed agent kernel with a declarative package format. Its
differentiator: inputs are hostile by default — untrusted text must never
expand what an agent can do, see, or send.

## Runtime vocabulary

- **Kernel**: the privileged runtime. Holds credentials, mediates every
  effect, owns workflows. Products never add bespoke kernel code.
- **Worker**: untrusted, sandboxed execution. **Orchestration** — model-driven
  control flow — lives in workers, permanently.
- **Workflow**: a declarative state machine. Workflows live in the kernel.
- **Package**: how a product exists in the kernel's vocabulary. Products are
  packages, never bespoke kernel code. **Lyra** is the first package; **Bell**
  is the second.

## The five promises

Citable by name. Correctness fixes serving a promise need no user story.

- **Ledger**: audited before effect, crash-safe, can testify later.
- **Permissions**: agents do only what was explicitly granted.
- **Switchboard**: channel-neutral ingress/egress; the owner is always
  reachable.
- **Immune system**: external content fills parameters, never gives
  instructions.
- **Hosting**: the assistant experience runs above the kernel, never inside
  it.

## The six users

Citable by name; definitions and failure modes in [STORIES.md](STORIES.md).

- **Lyra**: the trusted owner's assistant; authority grows only by explicit
  ratification.
- **Bell**: a multi-tenant AI receptionist; conversational principals are
  unauthenticated strangers.
- **Unattended workhorse**: no principal present; internal triggers.
- **Auditor**: adversarial reader of the record, months later.
- **Outside framework**: effect governance via the session surface without
  packages (hypothetical).
- **Delegating owner**: Lyra authors packages under kernel-enforced
  attenuation (fenced, do not build).

## Decision test

Land what serves two or more users; fold it where an existing ticket carries
those users; reject what serves one imagined story. Correctness fixes and the
five promises need no story. Normative form: [DIRECTION.md](DIRECTION.md).
