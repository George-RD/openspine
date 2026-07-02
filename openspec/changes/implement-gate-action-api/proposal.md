# Proposal: Implement gate action API

## Summary

Implement the first OpenSpine `gate()` boundary for evaluating effectful action requests against task grants.

## What Changes

This change adds a typed gate API that accepts action requests and returns allow, deny, or approval-required decisions.

## Why

The PRD states that every effectful action goes through `gate()`. This is the enforcement point that prevents agents from bypassing task grants.

## Affected layer

OpenSpine core.

## Authority sensitivity

Authority-sensitive.

This change implements the boundary that mediates runtime effects.

## Goals

- Define and implement typed action requests.
- Define and implement gate decisions.
- Check action requests against task grants.
- Emit audit events for gate decisions.
- Add tests for allowed, denied, and approval-required actions.

## Non-goals

- Do not implement real connectors.
- Do not implement approval UX.
- Do not implement model gateway.
- Do not implement external sends.

## Decision-log check

This change preserves the accepted decisions in `.raw/openspine-decision-log.md`.

If implementation reveals a need to weaken, reverse, or materially refine an accepted decision, update the decision log before completing the change.
