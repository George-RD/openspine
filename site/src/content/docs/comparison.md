---
title: How OpenSpine differs
description: OpenSpine, OpenClaw, and Hermes optimize different parts of the personal-agent problem.
---

## OpenSpine is the assistant system

You do not bring a finished assistant to OpenSpine today.

OpenSpine is the system you install. It includes:

- **Lyra**, the default assistant package you talk to;
- **the OpenSpine runtime**, which verifies requests, creates task permissions, holds credentials, checks actions, and records outcomes;
- **contained workers and workflows**, which do bounded work under those permissions.

The runtime is reusable, so other governed assistant packages can be built on it. A general plug-in interface for running OpenClaw, Hermes, or arbitrary third-party assistants inside OpenSpine is not shipped.

## The different starting point

OpenClaw and Hermes lead with what a personal agent can do: many channels, tools, skills, memory, terminal access, schedules, and automations. They also provide security controls such as pairing, allowlists, approvals, deny rules, and sandboxes.

OpenSpine starts with a narrower question:

> What must remain true after the model is wrong, manipulated, or replaced?

Its answer is to keep the authority boundary below the assistant:

- the worker does not receive raw connector credentials;
- external content can influence reasoning but cannot create permission;
- each task receives a short-lived grant built from all applicable constraints;
- every read, write, model call, connector call, and other effect crosses one gate;
- exact denials and approvals remain in force even when the prompt changes;
- capability growth follows a separate, reviewable lifecycle;
- public security claims point to named tests.

## Fair comparison

This is a product-emphasis comparison, not a claim that one project has no security model.

| | OpenClaw | Hermes | OpenSpine |
|---|---|---|---|
| Main story | Local, always-on personal assistant across many channels and devices | Self-improving agent with broad tools, memory, skills, schedules, and deployment backends | Trust-first personal AI with runtime-enforced task boundaries |
| Current strength | Channels, apps, tools, onboarding, mature assistant experience | Learning loop, terminal workflow, skills, memory, model flexibility | Authority outside the model, contained workers, one gate, test-backed claims |
| Typical safety controls | Pairing, allowlists, tool policies, approvals, sandbox modes, security audit | User authorization, dangerous-command approval, deny rules, container isolation, prompt scanning | Deterministic grants, kernel-held credentials, parameter binding, effect gate, digest-bound approval, audit |
| Current trade-off | Broad capability creates a large configuration and trust surface | Broad capability and self-modification require careful approval and containment choices | Far fewer user-facing workflows and more setup effort |
| Best fit now | You want a capable personal agent and will configure its trust boundary | You want a capable, learning, terminal-first agent | You need the assistant's authority to remain explicit and testable outside the model |

Primary sources:

- [OpenClaw README](https://github.com/openclaw/openclaw/blob/main/README.md)
- [OpenClaw security model](https://github.com/openclaw/openclaw/blob/main/docs/gateway/security/index.md)
- [Hermes README](https://github.com/NousResearch/hermes-agent/blob/main/README.md)
- [Hermes security model](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/user-guide/security.md)
- [Hermes skills model](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/user-guide/features/skills.md)

## A real failure scene

Suppose you ask an assistant to reply to one email thread. That thread contains a hidden or explicit instruction telling the agent to read other mail and forward it.

A prompt-level response is to tell the model to ignore instructions inside email. A tool-level response is to ask before a dangerous command. A sandbox can reduce host damage.

OpenSpine changes the task itself:

1. The owner request is verified.
2. The selected thread is bound to a single-use token.
3. The runtime creates a grant for that thread and workflow.
4. The contained worker receives the thread, but not Gmail credentials.
5. A request for another thread is outside the grant and is denied.
6. Draft creation requires approval of the exact payload and target.
7. Email sending is denied by global policy.

The model may still produce a bad draft. It does not gain a route to more mail because the email told it to.

## What OpenSpine does not yet match

OpenSpine is an alpha. It does not currently match the assistant breadth or onboarding of OpenClaw or Hermes.

Today, the public Lyra path requires Telegram, Gmail OAuth, a model provider, Docker, and a thread ID copied from Gmail. The working result is selected-thread reply drafting and approved Gmail draft creation. Sending remains blocked.

The runtime has landed machinery for workers, workflows, skills, memory overlays, standing rules, task tracking, reflection, and disclosure policy. That does not mean each capability has a complete owner-facing Lyra workflow.

## The intended end state

OpenSpine is working toward a personal AI that can do useful internal work freely, ask only at a real effect boundary, learn standing rules after explicit confirmation, and show receipts when asked.

The user-facing ambition is a chief of staff. The architectural commitment is that the chief of staff never becomes the authority source.