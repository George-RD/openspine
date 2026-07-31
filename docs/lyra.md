# Lyra

Lyra is the default owner-facing assistant package built into the OpenSpine system.

For a user, the simple model is:

```text
OpenSpine is the system you install.
Lyra is the assistant you talk to.
The OpenSpine runtime decides what each task may do.
```

The product promise is broader than one guarded task:

> **Let Lyra earn more responsibility, one job at a time.**

Lyra can learn the job. It cannot promote itself.

OpenSpine is not currently a compatibility layer where a finished OpenClaw, Hermes, or arbitrary third-party assistant is dropped into the runtime. The runtime is reusable, so other governed assistant packages can be built on it, but Lyra is the supported product path today.

## Package and runtime

Lyra is a declarative composition of agents, routes, workflows, capability packs, policies, templates, persona overlays, and memory scopes that the runtime loads and constrains.

The source package lives in [`artifacts/lyra`](../artifacts/lyra/). Its persistent entry agent is `main_assistant_agent`. The package also contains bounded workers for tasks such as email drafting and reflection, plus the routes and workflows needed to reach them.

A source checkout already uses Lyra by default because `Config::lyra_dir` defaults to `artifacts/lyra`.

## How Lyra is meant to grow

The intended owner experience resembles managing a good employee:

1. **Delegate one clear job.** The runtime gives Lyra and any commissioned worker only the context and authority needed for that task.
2. **Let internal work proceed.** Lyra should ask only at a real effect or disclosure boundary, not for every intermediate thought.
3. **Review the result.** Corrections, exceptions, and approvals remain tied to the work that produced them.
4. **Turn repetition into a proposal.** Repeated approvals, corrections, and stated preferences can produce versioned proposals for reusable routines, standing rules, preferences, or other governed artifacts.
5. **Approve the responsibility.** The owner may narrow, edit, reject, activate, pause, expire, or revoke the proposal.
6. **Reduce future interruption.** Matching work can proceed with the approved responsibility while each new task still receives its own bounded grant.
7. **Return drift for review.** Changed targets, exhausted budgets, saturation, expiry, or revocation remove the shortcut rather than silently widening it.

The runtime already contains much of this substrate:

- contained workers and structured result chokepoints;
- durable workflows, timers, task-board objects, and replay;
- versioned artifact proposal, evaluation, approval, activation, retirement, and revocation;
- standing rules with budgets, expiry, drift handling, and dark-window defaults;
- reflection that can propose changes but cannot directly mutate or activate kernel state;
- typed persona and preference artifacts kept separate from authority.

The complete owner-facing progression is not shipped yet. [Issue #123](https://github.com/George-RD/openspine/issues/123) tracks the product loop that joins these pieces without making the owner edit YAML or reason about runtime ontology.

## What Lyra does today

The public alpha exposes two first-rung paths.

### Local terminal conversation

`openspine chat` provides a direct line-oriented conversation path, with `--once` for smoke tests and scripts. Each message becomes a verified owner event, receives a signed task grant, and runs through the contained model path, action gate, artifact store, and audit trail.

### Selected Gmail draft

The verified Telegram workflow:

1. accepts `/draft <gmail_thread_id>` from the verified owner;
2. binds the task to that selected Gmail thread;
3. commissions a contained drafting worker;
4. previews the proposed reply in Telegram;
5. creates the exact Gmail draft only after digest-bound approval;
6. denies email sending by global runtime policy.

These are narrow trust proofs, not the full employee-like experience described above. The first task boundary is working; the seamless progression from repeated delegation to reusable responsibility remains product work.

## Declarative model

Lyra's configuration states desired behaviour:

- which agent receives an event;
- which workflows and workers may be selected;
- which tools each agent was designed to use;
- which memory classes and scopes it may read;
- which actions require approval or are denied;
- which output channels are valid.

The declarations do not grant authority by themselves. The kernel calculates a bounded task grant from the applicable route, agent, workflow, capability pack, policy, caveat, approval, standing-rule input, and runtime constraints.

Reusable capability and authority also remain declarative and versioned. A model may suggest a change, but the suggestion does not become active merely because it appeared in a prompt or memory. This is intentionally closer to a desired-state system than to asking the model to police itself.

## Personality versus memory versus authority

OpenSpine keeps four concerns separate:

1. **Persona**: learnable behavioural guidance such as directness, discretion, continuity, and recommendation style.
2. **Memory**: typed, scoped artifacts carrying selected preferences, facts, commitments, and workflow state with provenance.
3. **Authority**: policies, capability packs, approvals, caveats, standing rules, and the task grant enforced by the kernel.
4. **Prompts**: task instructions assembled only after authority is resolved.

A `soul.md` can be a useful authoring surface, but it becomes dangerous when personality, durable memory, and permissions are collapsed into one mutable file. Lyra therefore has a human-readable `PERSONA.md`, while runtime personality and memory remain structured overlays that can evolve without silently widening permissions.

A preference such as “keep replies brief” may become durable guidance. It may not become “send replies automatically.” Learning how the owner likes work done remains separate from permission to cause an effect.

## Installation direction

The intended user-facing model is:

```text
openspine install lyra
openspine use lyra
openspine run
```

That native package store and resolver are not shipped yet. [Issue #117](https://github.com/George-RD/openspine/issues/117) tracks the transactional, versioned, and auditable installer required to make the product model match the architecture.

Until then, the source package and `lyra_dir` configuration remain the implementation interface.
