# OpenSpine progressive delegation positioning

Date: 2026-07-31
Status: Product-positioning correction for PR #122
Trigger: Direct product-owner feedback that the first-rung Gmail proof made OpenSpine look careful but incapable

## Executive decision

OpenSpine should not lead with the narrow Gmail workflow or with the runtime boundary by itself.

The product promise is:

> **Let your AI earn more responsibility. One job at a time.**

The crucial guardrail is:

> **Lyra can learn the job. It cannot promote itself.**

OpenSpine is an employee-like personal AI system whose capability and autonomy are intended to grow as the owner delegates more work. Bounded task grants, credential separation, and action gates make that growth governable; they are not the whole offer.

## What the previous copy got wrong

The previous version corrected an architecture-first explanation but then put this current proof in the first viewport:

> Choose one Gmail thread. Lyra drafts a reply. Only the exact draft you approve is created. Sending stays blocked.

That statement is true. Its position was wrong.

Placed beside the hero, it defined the product as a constrained Gmail drafting tool. It emphasised what Lyra cannot do before explaining how the system is meant to become more capable.

The current workflow should remain visible as proof of the first task boundary, but lower in the page and explicitly labelled as the first rung rather than the product ceiling.

## Product model

The intended working relationship is:

1. The owner delegates one clear job.
2. Lyra completes internal work within a bounded task.
3. It asks only at a real effect or disclosure boundary.
4. The owner reviews the result, correction, and exception.
5. Repeated approvals, corrections, and preferences can produce a reviewable proposal.
6. The owner approves, narrows, edits, rejects, pauses, expires, or revokes the reusable responsibility.
7. Future matching work needs less interruption.
8. Drift, changed context, budget saturation, or revocation returns control to the owner.

The owner should experience this like managing a good employee, not like editing a permission ontology.

## Architecture support already present

The repository already contains much of the substrate for this model:

- Lyra is a declarative composition of agents, routes, workflows, capability packs, policies, templates, persona overlays, and memory scopes.
- Every live worker receives a bounded task grant rather than raw connector credentials.
- Contained workers, durable workflows, task boards, timers, replay, and structured results have landed.
- Skills and other artifacts use versioned proposal, evaluation, approval, activation, retirement, and revocation paths.
- Standing rules are versioned, budgeted, expiring, and revocable composition inputs. They do not replace or widen the task grant.
- The reflection miner can propose corrections, preferences, standing-rule candidates, and consolidation changes, but it cannot mutate or activate kernel state directly.
- Persona, memory, authority, and prompts remain separate concerns.

## Product gap

The runtime pieces are not yet joined into one seamless owner-facing delegation loop.

That gap matters because both bad messages are possible:

- show only the Gmail proof and OpenSpine appears incapable;
- advertise all landed runtime machinery as a finished Lyra experience and the copy overclaims.

The correct public distinction is:

1. **Product promise:** employee-like capability that grows through delegation.
2. **Runtime landed:** machinery that makes governed growth possible.
3. **Wired into Lyra:** owner-facing workflows proven end to end.
4. **Product surface missing:** the natural interaction that joins the pieces.

[Issue #123](https://github.com/George-RD/openspine/issues/123) owns the missing progressive delegation loop. Issue #118 owns the first useful task and onboarding. Issue #119 owns the truthful capability map and owner-path proof standard.

## Message hierarchy

A first-time reader should learn these points in order:

1. A personal AI can become more useful as the owner delegates more work.
2. More autonomy appears through reviewed responsibility, not broad access up front.
3. Lyra may propose new routines, rules, preferences, or capabilities, but cannot activate them itself.
4. Every current task remains bounded, contained, gated, and recorded.
5. The current alpha exposes local chat and one guarded Gmail drafting flow.
6. More governed runtime machinery exists than the current Lyra product surface exposes.
7. OpenClaw and Hermes offer far more current breadth; OpenSpine makes a different architectural and product trade.

## Approved landing direction

### Badge

**Self-hosted personal AI · grows by delegation · alpha**

### Headline

**Let your AI earn more responsibility.\nOne job at a time.**

### Supporting copy

**OpenSpine comes with Lyra, the assistant you talk to. Give it one clear job, review the result, then let repeated work become a responsibility you can inspect, limit, pause, or remove. Lyra can learn the job. It cannot promote itself.**

### Recognition line

**Seen what OpenClaw or Hermes can do? OpenSpine is built for what comes next: delegate, review, then let useful autonomy grow through clear responsibilities.**

### Guardrail line

**New routines and permissions stay inactive until you approve them. Pause or revoke them later.**

### Primary action

**See how responsibility grows.**

## Current proof placement

The Gmail path should appear as a concrete first-rung example after the growth model:

> Today, Lyra can chat locally and complete one guarded Gmail drafting flow. Those paths prove the first task boundary. Underneath them, OpenSpine already has contained workers, declarative workflows, standing rules, and governed learning.

The page must then state that the complete owner-facing delegation loop is not shipped yet.

## Claim guardrails

- Do not say the progressive delegation loop is already complete.
- Do not describe runtime-landed machinery as available through Lyra without an owner-path test.
- Do not imply Lyra can activate a skill, standing rule, preference, workflow, or capability by itself.
- Do not collapse preference learning into permission growth.
- Do not hide the current alpha's narrow breadth or technical setup.
- Do not attack OpenClaw or Hermes as careless or without security controls.
- Do not let the current Gmail proof define the whole product.

## Roadmap implication

The progressive delegation loop is not copy polish. It is a missing product milestone.

The owner-facing implementation should demonstrate one complete positive progression:

1. delegate a recurring task;
2. approve a bounded effect;
3. observe a proposal after repeated matching work;
4. review its targets, actions, budget, and expiry in ordinary language;
5. activate it;
6. complete the next matching task with less interruption;
7. fall back on changed context or exhausted budget;
8. pause or revoke it and prove the shortcut disappears.

## Evaluation correction

The previous evaluation treated current proof proximity as an unconditional trust improvement. That was incomplete.

For an architecture-led product, proof must support the dream outcome rather than replace it. A narrow proof placed too early can reduce perceived capability even while increasing factual specificity.

Future copy gates should separately ask:

1. Does the reader understand the product's desired future working relationship?
2. Does the proof make that relationship believable?
3. Has the proof accidentally become the apparent capability ceiling?
4. Are shipped product breadth, landed runtime support, and roadmap direction clearly separated?
