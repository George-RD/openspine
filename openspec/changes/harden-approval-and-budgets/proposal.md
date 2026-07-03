# Proposal: Harden approval and budgets

## Summary

Close three verified hardening gaps in the shipped Phase 1–3 slices: a
truncated Telegram preview could still carry an approval button bound to
content the owner never saw in full; `GrantLimits.max_model_calls` and
`max_artifacts` are composed and served but never enforced; and task
tokens are stored in plaintext in SQLite with no retention sweep.

## What Changes

- `dispatch_lyra_preview` now builds the full draft text before truncating
  it for Telegram, and refuses to propose an approval at all when the
  shown text had to be cut short (WYSIWYS).
- `POST /v1/model/generate` and the shell-initiated artifact-put call
  sites now enforce `max_model_calls` and `max_artifacts` kernel-dispatch-
  side, denying `limit_exceeded` once a grant's budget is spent.
- `task_grants.task_token` now stores a hash of the bearer token, never
  the plaintext; the token is also redacted from the persisted
  `grant_json` blob. Expired grants are swept on every new grant insert.
- `notify_owner_best_effort`'s kernel-originated Telegram sends are now
  audited (`owner.notified`) even though they remain ungated.
- Operator docs (`.env.example`, `compose.yaml`, README) gain the Gmail
  env var section and a documented (opt-in, inactive) `docker-socket-proxy`
  hardening path.

## Why

A full review against the PRD, decision log, and openspec coverage found
these three gaps were real and exploitable within the existing shipped
slices, not merely theoretical: an owner could tap Approve on unseen
tail content; a misbehaving or compromised agent could exceed its
resource budget indefinitely; and a leaked SQLite file would hand out
live bearer tokens.

## Affected layer

OpenSpine core.

## Authority sensitivity

Authority-sensitive. This change touches approval binding, grant budget
enforcement, and task-token storage — all part of the runtime authority
boundary.

## Goals

- Approval must bind only what the owner was actually shown.
- Grant budgets (`max_model_calls`, `max_artifacts`) must be enforced at
  runtime, not merely advertised.
- Task tokens must be unrecoverable from a leaked store.
- Kernel-originated owner notifications must remain auditable even
  though they stay outside `gate()`.

## Non-goals

- Do not change the shape or meaning of `GrantLimits` fields.
- Do not gate `notify_owner_best_effort` through `gate()` — it stays a
  documented, audited, trusted-path carve-out.
- Do not migrate existing plaintext dev-database rows; short-lived task
  tokens (≤180s) make this unnecessary.
- Do not implement the artifact-lifecycle slice (`artifact.propose`) —
  that is a separate, later change.

## Decision-log check

This change was checked against `.raw/openspine-decision-log.md`. It
does not reverse or weaken any accepted decision; it adds D-045, D-046,
and D-047 to record the WYSIWYS fix, budget-enforcement placement, and
token-hashing/sweep decisions respectively.
