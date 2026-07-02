# Proposal: Implement digest-bound draft approval

## Summary

Implement digest-bound approval for creating a Gmail draft from a reviewed email reply preview.

## What Changes

This change adds approval records that bind the exact reviewed payload and target before allowing Gmail draft creation.

It allows creating a Gmail draft only after the owner approves the exact immutable draft artifact and target.

## Why

The PRD excludes final send from early phases, but draft creation is useful once preview and approval are safe. Approval must be digest-bound so an agent cannot show draft A and execute draft B.

## Affected layer

Both OpenSpine core and Lyra product.

## Authority sensitivity

Authority-sensitive.

This change allows an externally visible mailbox mutation: creating a draft.

## Goals

- Store draft preview as immutable artifact.
- Record payload digest and target digest.
- Require owner approval for draft creation.
- Invalidate approval if payload or target changes.
- Create Gmail draft only after gate validates approval.
- Continue denying final email send.

## Non-goals

- Do not send email.
- Do not approve automatically.
- Do not allow recipient/thread mutation after approval.
- Do not support non-Gmail providers yet.

## Decision-log check

This change preserves the accepted decisions in `.raw/openspine-decision-log.md`.

If implementation reveals a need to weaken, reverse, or materially refine an accepted decision, update the decision log before completing the change.
