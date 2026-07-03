---
title: Why OpenSpine
description: The problem OpenSpine solves and the bet it makes.
---

## The problem

Other tools focus on what an AI agent can do. They add more tools, more connectors, and more freedom. This leads to security failures. A bad prompt can make the agent take actions you did not want. For example, a prompt-injected email could cause the agent to run a tool you never allowed. Trying to add rules after a failure is very hard. Rules must be built into the foundation, not added later.

## The bet

OpenSpine puts safety first. The base layer (substrate) owns the rules, not the AI model. An agent has no trust by default. We follow these rules:

- **Identity alone is not enough.** A verified sender or a logged-in account does not grant trust. They are only inputs. They do not replace rules.
- **Rules are combined, not just assigned.** Rules from routes, agents, and policies are merged. Everything is blocked unless a rule explicitly allows it.
- **Every action goes through a gate.** Reading data, calling AI models, and writing data must pass a single check. We call this the gate (effectful action). It allows, denies, or asks for your approval.
- **External data is never an instruction.** The agent treats emails or web pages as raw text, never as commands. The model gateway wraps this data so the AI model cannot be tricked.

## Safe rules for changing behavior

Agents can suggest new behaviors. They can suggest new routes or narrower rules. But they can never turn them on.

The rule lifecycle works like this:
1. The agent suggests a new rule.
2. The system checks the rule format and saves it as proposed.
3. You see an approval request on Telegram. The request is locked to the exact rule you see.
4. You tap Approve to turn the rule on.

Nothing turns itself on. We do not try to guess if a rule is safe. You must approve every single change.

## What OpenSpine does not do on purpose

To keep things safe, OpenSpine does three things on purpose:
- We do not have a store to download new tools. The agent cannot earn more trust over time.
- The agent is blocked from sending emails. Lyra can only draft emails, never send them.
- The agent cannot change its prompt templates while running. This prevents prompt injection attacks.

Every limit is chosen on purpose. You can read our reasoning in the [decision log](/openspine/decisions/).
