# Proposal: Define core runtime schemas

## Summary

Define the first canonical OpenSpine runtime schemas without implementing runtime behavior.

## What Changes

This change adds schema definitions and requirements for the core OpenSpine runtime objects:

- event envelope;
- identity record;
- identity resolution output;
- route artifact;
- route resolution result;
- agent manifest;
- workflow manifest;
- capability pack;
- authority composition input/output;
- task grant;
- action request;
- gate decision;
- approval record;
- selection token;
- model request;
- audit event;
- artifact reference.

It does not implement live runtime execution.

## Why

The PRD defines OpenSpine as a governed runtime substrate. Before implementing Telegram, Gmail, model gateway, gate(), or task grants, the core nouns need stable schemas.

Without schemas, agents may implement inconsistent structures and blur the boundary between proposed artifacts, runtime artifacts, and live authority.

## Affected layer

OpenSpine core.

## Authority sensitivity

Authority-sensitive.

This change defines the artifacts that later participate in runtime authority. It must not activate any authority by itself.

## Goals

- Convert core PRD concepts into versioned schemas.
- Make runtime authority objects explicit.
- Keep OpenSpec artifacts separate from OpenSpine runtime artifacts.
- Provide a typed foundation for later implementation slices.

## Non-goals

- Do not implement authority composition.
- Do not implement gate().
- Do not implement Telegram or Gmail connectors.
- Do not implement model gateway.
- Do not implement audit storage.
- Do not activate runtime routes, workflows, or capability packs.

## Decision-log check

This change preserves the accepted decisions in `.raw/openspine-decision-log.md`.

If implementation reveals a need to weaken, reverse, or materially refine an accepted decision, update the decision log before completing the change.
