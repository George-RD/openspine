---
title: Decisions
description: How the decision-log practice keeps the architecture's reasoning legible and catches real bugs.
---

## Why a decision log

Code shows *what* the system does. A decision log records *why* it does
that instead of the alternative — the trade-off considered, the rejected
option, and the condition under which the choice would be revisited. The
practice matters because "why" is exactly what a future change needs to
know before it can safely touch something, and exactly what a code review
alone cannot recover once the original reasoning is gone from working
memory.

OpenSpine's log
([`.raw/openspine-decision-log.md`](https://github.com/George-RD/openspine/blob/main/.raw/openspine-decision-log.md))
has grown to 49 entries (D-001–D-049) across the design and implementation
of every slice shipped so far. Every entry follows the same shape:
**Decision**, **Rationale**, **Consequences**, **Would change if** — that
last section is the part most decision logs skip, and the part that turns
a historical note into something a future author can actually act on.

## A worked example: catching a real approval bypass

D-034 is the clearest case of the practice earning its keep. The PRD's own
text specified two different spellings for the Gmail draft-creation action:
a bare `email.create_draft` in one section, and a qualified
`email.create_draft:after_payload_approval` in another. Action identifiers
in OpenSpine are exact-match strings with no wildcard semantics (D-033) —
so keeping the qualified spelling would have put it into an agent's
candidate `allowed_actions` set as a **plain allow**, with no
corresponding entry in any capability pack's `approval_required` list.
That silently bypasses the entire digest-bound approval mechanism (D-011)
the qualifier's own name claims to require — an agent could create a Gmail
draft with no owner approval at all, simply because two names for "the
same" action were, to `gate()`, two unrelated actions.

The decision log entry is what forced this comparison to happen explicitly
before implementation, rather than being discovered as a live bug: D-034
picks the bare spelling as canonical, records *why* the qualified spelling
is unsafe rather than merely unused, and gives `openspine-authority`'s test
suite a permanent regression check (the composed selected-thread-email
task grant must contain no `create_draft` variant in `allowed_actions`, and
exactly `email.create_draft` in `approval_required_actions`).

## Reading the log

Each entry stands alone — read the one relevant to the code you're
touching rather than the whole file front to back. If a change reveals a
need to weaken, reverse, or materially refine an accepted decision, the
house rule is to update the log *before* the code lands, not after: the
[per-change ceremony](https://github.com/George-RD/openspine/blob/main/openspec/conventions.md)
requires every OpenSpec proposal to check the decision log first.
