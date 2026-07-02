# Proposal: Implement Telegram owner control slice

## Summary

Implement the first Lyra owner-control workflow on OpenSpine using Telegram.

## What Changes

This change adds the minimum usable owner-control lane:

- Telegram bot connector;
- configured owner Telegram ID verification;
- `telegram.owner.message` event envelope;
- owner identity binding;
- deterministic route to `main_assistant_agent`;
- owner-control capability pack;
- owner-control task grant;
- status response;
- setup/proposal stubs;
- audit events.

## Why

The PRD defines Telegram owner control as the first usable Lyra product interface. It proves OpenSpine can support a governed assistant without giving the assistant broad external data access.

## Affected layer

Both OpenSpine core and Lyra product.

## Authority sensitivity

Authority-sensitive.

The slice creates a verified owner control lane. It must not accidentally grant external communication, filesystem, network, infrastructure, or email authority.

## Goals

- Let the verified owner message the Lyra Telegram bot.
- Normalize messages into OpenSpine events.
- Verify configured owner Telegram ID.
- Route to `main_assistant_agent`.
- Issue owner-control task grant.
- Allow low-risk status/setup/proposal actions.
- Deny broad external access by default.

## Non-goals

- Do not implement Gmail access.
- Do not implement inbox reads.
- Do not implement email drafting.
- Do not implement infrastructure actions.
- Do not expose secrets to agent/model context.

## Decision-log check

This change preserves the accepted decisions in `.raw/openspine-decision-log.md`.

If implementation reveals a need to weaken, reverse, or materially refine an accepted decision, update the decision log before completing the change.
