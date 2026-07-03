---
title: Why OpenSpine
description: The problem OpenSpine solves and the bet it makes.
---

## The problem

Capability frameworks — OpenClaw-style assistants, LangGraph-style
orchestration, most agent SDKs — optimise what an agent *can* do: more
tools, more connectors, more autonomy. Their failure mode is an authority
failure, not a capability one: a prompt-injected email turns into an
outbound action, a tool call nobody scoped, an agent that widened its own
permissions because nothing was watching. Bolting a policy layer on top of
a capability-first design after the fact is exactly how those failures
happen — the enforcement point is an afterthought, not the foundation.

## The bet

OpenSpine's bet is that the substrate, not the model and not the agent
framework, owns authority. Concretely:

- **Identity is not authority.** A verified sender, an authenticated
  connector, or a matched account role each grant nothing by themselves.
  They are inputs to authority composition, never a substitute for it.
- **Authority is composed, not assigned.** A route, agent manifest,
  workflow, capability pack, or policy is a *candidate* — deny-by-default
  intersection across all of them produces the one task grant a running
  agent holds. There is no code path that skips composition.
- **Every effect is gated.** Reads, model calls, and connector writes all
  pass through one `gate()` boundary before anything happens. A decision
  is allow, deny, or approval-required — never a silent side effect.
- **External content is data, never instruction.** A fetched email, a web
  page, an inbound message — none of it is trusted as an instruction to
  the model, and the model gateway wraps it with a per-call randomised
  delimiter specifically so it cannot spoof its way out of that boundary.

## Dynamic behaviour easy, dynamic authority hard

Agents should be free to propose new behaviour — a new route, a new
workflow, a narrower policy. What they should never be free to do is
*activate* it. The artifact-lifecycle slice makes this concrete: an agent
calls `artifact.propose` with a declarative YAML artifact; the kernel
schema-validates it and persists it as `proposed`; the owner sees a
Telegram approval button digest-bound to the exact YAML and to
`{kind, artifact_id, version}`; only a tap on that exact button activates
it into the live registry and an on-disk overlay. Nothing self-activates.
There is no widening-detection heuristic that lets a "safe-looking"
proposal skip the button — every proposal gets the same explicit approval,
on purpose, because deciding some proposals are safe enough to auto-approve
is itself an authority decision no one has designed yet.

## What OpenSpine deliberately does not do

- No artifact marketplace, and no autonomy ladder that lets an agent earn
  broader trust over time just by behaving well.
- No `email.send` — Lyra drafts, it never sends, regardless of grant or
  approval state.
- No prompt-template proposals at runtime — a template changes the
  model's *instruction* surface, not just its authority, and that is a
  different, larger injection-escalation risk than the artifact-lifecycle
  slice is scoped to close.

The restraint is the pitch: every one of these is a decision recorded in
the [decision log](/decisions/), not a gap nobody noticed.
