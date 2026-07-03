# Proposal: Backfill implemented capability specs

## Summary

Add OpenSpec capability specs for three subsystems (`model-gateway`,
`audit-artifact-store`, `shell-containment`) that were implemented inside
earlier build-plan slices but never got a standalone spec, and restore
two dev-process requirements that were silently dropped when
`openspine-development-process`'s canonical spec was condensed from its
original archived delta.

## What Changes

- New `openspec/specs/model-gateway/spec.md`, `audit-artifact-store/spec.md`,
  and `shell-containment/spec.md`, each derived from existing code,
  decision-log entries, and (where one exists) the enforcing test.
- `openspine-development-process`'s canonical spec regains: the
  "`tasks.md` grants no runtime access" scenario, and the
  "archive MUST preserve rationale" bullet list.
- `openspec/openspine-change-sequence.md` gains a "Backfilled" subsection
  under Completed, moving the three now-specced ids out of "Later
  changes."
- A new ADDED requirement on `openspine-development-process`: a change
  implementing a security-load-bearing subsystem must add that
  subsystem's capability spec in the same change, closing the loophole
  this proposal itself is repaying.

## Why

`openspec validate --all --strict` covered only 7 of the 10 capabilities
the shipped code actually implements. Specs describe authority machinery;
a capability the spec system doesn't know about cannot be reviewed for
drift, and a future change could silently regress model-gateway,
audit, or containment guarantees with no spec to fail against.

## Affected layer

Development tooling and the spec system. No runtime code changes.

## Authority sensitivity

Authority-sensitive: these specs describe authority machinery (model
gateway credential isolation, audit hash-chaining, shell containment),
even though this change makes no runtime edits.

## Goals

- Every requirement in the three new specs must already hold in code.
- Cite the enforcing test in each scenario where one exists.
- Restore the two dev-process requirements verbatim from the archived
  delta, not paraphrased.
- Close the loophole with a forward-looking process requirement.

## Non-goals

- Do not change any runtime behaviour.
- Do not add new tests for behaviour that isn't already tested (`docs/threat-claims.md`
  in a later change, `add-threat-claims-register`, is where any real test
  gaps get tracked and closed).

## Decision-log check

This change was checked against `.raw/openspine-decision-log.md`. It does
not reverse or weaken any accepted decision; it adds D-049 to record why
these specs were backfilled and to make the omission structurally harder
to repeat.
