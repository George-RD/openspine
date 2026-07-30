# Lyra

Lyra is the default owner-facing assistant package built into the OpenSpine system.

For a user, the simple model is:

```text
OpenSpine is the system you install.
Lyra is the assistant you talk to.
The OpenSpine runtime decides what each task may do.
```

OpenSpine is not currently a compatibility layer where a finished OpenClaw, Hermes, or arbitrary third-party assistant is dropped into the runtime. The runtime is reusable, so other governed assistant packages can be built on it, but Lyra is the supported product path today.

## Package and runtime

Lyra is a declarative composition of agents, routes, workflows, capability packs, policies, templates, persona overlays, and memory scopes that the runtime loads and constrains.

The source package lives in [`artifacts/lyra`](../artifacts/lyra/). Its persistent entry agent is `main_assistant_agent`. The package also contains bounded workers for tasks such as email drafting and reflection, plus the routes and workflows needed to reach them.

A source checkout already uses Lyra by default because `Config::lyra_dir` defaults to `artifacts/lyra`.

## What Lyra does today

The public alpha uses a verified Telegram channel as owner control. Its first guarded workflow:

1. accepts `/draft <gmail_thread_id>` from the verified owner;
2. binds the task to that selected Gmail thread;
3. commissions a contained drafting worker;
4. previews the proposed reply in Telegram;
5. creates the exact Gmail draft only after digest-bound approval;
6. denies email sending by global runtime policy.

This is a narrow trust proof, not the full chief-of-staff experience described by the agent-OS vision.

## Declarative model

Lyra's configuration states desired behaviour:

- which agent receives an event;
- which workflows and workers may be selected;
- which tools each agent was designed to use;
- which memory classes and scopes it may read;
- which actions require approval or are denied;
- which output channels are valid.

The declarations do not grant authority by themselves. The kernel calculates a bounded task grant from the applicable route, agent, workflow, capability pack, policy, caveat, approval, and runtime constraints. This is intentionally closer to Nix's desired-state model than to a prompt that asks a model to police itself.

## Personality versus memory

OpenSpine keeps four concerns separate:

1. **Persona**: learnable behavioural guidance such as directness, discretion, continuity, and recommendation style.
2. **Memory**: typed, scoped artifacts carrying selected preferences, facts, commitments, and workflow state with provenance.
3. **Authority**: policies, capability packs, approvals, caveats, and the task grant enforced by the kernel.
4. **Prompts**: task instructions assembled only after authority is resolved.

A `soul.md` can be a useful authoring surface, but it becomes dangerous when personality, durable memory, and permissions are collapsed into one mutable file. Lyra therefore has a human-readable `PERSONA.md`, while runtime personality and memory remain structured overlays that can evolve without silently widening permissions.

## Installation direction

The intended user-facing model is:

```text
openspine install lyra
openspine use lyra
openspine run
```

That native package store and resolver are not shipped yet. [Issue #117](https://github.com/George-RD/openspine/issues/117) tracks the transactional, versioned, and auditable installer required to make the product model match the architecture.

Until then, the source package and `lyra_dir` configuration remain the implementation interface.