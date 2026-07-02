# Proposal: Implement authority composition

## Summary

Implement the first OpenSpine authority composer and task-grant materialization logic.

## What Changes

This change implements the deny-by-default authority composition rules from the PRD and decision log.

It adds logic that takes verified event context, identity resolution output, route policy, global policy, agent manifest, workflow, capability pack, lane, connector, account role, and session policy as inputs, then materializes a bounded task grant.

## Why

The task grant is the final runtime authority object. Without authority composition, agents would either receive no useful capabilities or accidentally receive broad implicit permissions.

## Affected layer

OpenSpine core.

## Authority sensitivity

Authority-sensitive.

This change directly implements runtime authority construction.

## Goals

- Implement deny-by-default authority composition.
- Preserve explicit-deny precedence.
- Preserve approval-required precedence.
- Materialize task grants only from intersected authority sources.
- Add tests for conflicts, denials, and approval-required outcomes.

## Non-goals

- Do not implement gate() execution.
- Do not implement real connectors.
- Do not implement Telegram or Gmail.
- Do not implement model gateway.
- Do not implement UI approval.

## Decision-log check

This change preserves the accepted decisions in `.raw/openspine-decision-log.md`.

If implementation reveals a need to weaken, reverse, or materially refine an accepted decision, update the decision log before completing the change.
