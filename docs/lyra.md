# Lyra

Lyra is the default owner-facing agent package built on OpenSpine.

OpenSpine is the governed runtime. Lyra is a declarative composition of agents,
routes, workflows, capability packs, policies, templates, persona overlays, and
memory scopes that the runtime loads and constrains.

## What exists now

The source package lives in [`artifacts/lyra`](../artifacts/lyra/). Its persistent
entry agent is `main_assistant_agent`. The package also contains bounded workers
for tasks such as email drafting and reflection, plus the routes and workflows
needed to reach them.

A source checkout already uses Lyra by default because `Config::lyra_dir`
defaults to `artifacts/lyra`.

## Declarative model

Lyra's configuration states desired behavior:

- which agent receives an event;
- which workflows and workers may be selected;
- which tools each agent was designed to use;
- which memory classes and scopes it may read;
- which actions require approval or are denied;
- which output channels are valid.

The declarations do not grant authority by themselves. The kernel calculates a
bounded task grant from the applicable route, agent, workflow, capability pack,
policy, caveat, approval, and runtime constraints. This is intentionally closer
to Nix's desired-state model than to a prompt that asks a model to police itself.

## Personality versus memory

OpenSpine keeps four concerns separate:

1. **Persona** — learnable behavioral guidance such as directness, discretion,
   continuity, and recommendation style.
2. **Memory** — typed, scoped artifacts carrying selected preferences, facts,
   commitments, and workflow state with provenance.
3. **Authority** — policies, capability packs, approvals, caveats, and the task
   grant enforced by the kernel.
4. **Prompts** — task instructions assembled only after authority is resolved.

A `soul.md` can be a useful authoring surface, but it becomes dangerous when
personality, durable memory, and permissions are collapsed into one mutable file.
Lyra therefore has a human-readable `PERSONA.md`, while runtime personality and
memory remain structured overlays that can evolve without silently widening
permissions.

## Installation direction

The intended user-facing model is:

```text
openspine install lyra
openspine use lyra
openspine run
```

The first release of Lyra establishes the package boundary and manifest. A
native package store and CLI resolver should follow as a separate change so the
installer can be versioned, transactional, and auditable rather than implemented
as an unsafe directory copy.
