# Proposal: Implement identity store and principal

## Summary

Introduce a first-class `Principal` record and a persisted identity store behind
a read-only `IdentityResolver` seam, so that pipeline **authority composition
consumes a resolved `principal_id` instead of the owner config string**. v1
enforces exactly one principal — the owner — at the storage layer. Identity
binding ("my wife's number is this") becomes an audited, **owner-approved**
action requiring an authenticated owner-principal context; it is never
agent-triggered. Unknown claims never auto-bind. `Identity` and `Principal`
keep zero authority fields (D-006): a counterparty with rich standing rules is
still not a principal.

## What Changes

- A new `Principal` schema (authority-free; `id`, `identity_id`, `is_owner`,
  `schema_version`) and persisted identity tables (`principals`, `identities`,
  `identity_identifiers`) in the kernel's SQLite store.
- A read-only `IdentityResolver` seam replaces the hardcoded
  `resolve_owner_identity`: the owner is the fast path (yields
  `principal_id`); a bound counterparty resolves to an `Identity` but **no**
  `principal_id`; an unknown identifier resolves to `RelationshipKind::Unknown`,
  confidence `0`, and is never bound. The resolver performs no writes.
- `IdentityResolution` gains `principal_id: Option<Ulid>` — `Some` only on the
  owner fast path. A counterparty or unknown can never yield a principal, so
  binding an identity cannot implicitly promote it to `Principal`.
- Authority composition consumes `principal_id`: `AuthorityInput.user` becomes
  `principal_id: Ulid`, and composition fails closed when no principal is
  resolved. The owner config Telegram id remains only the channel
  *authentication* signal (`verify_update`), no longer the composition input.
- v1 single-owner is a storage invariant: a partial unique index guarantees at
  most one `is_owner` principal; bootstrap is transactional and idempotent.
- Owner-asserted identity binding is an audited (`identity.bound`) store
  operation gated on an authenticated owner-principal context; it is absent
  from every capability pack and the shell action-handler registry, so the
  agent path can never reach it.

## Why

AD-146 (`.raw/openspine-agentos-design-log.md:535`): "`Principal` becomes a
first-class record; v1 enforces exactly one (`is_owner`); the owner stops being
a config string (today the pipeline wires `state.owner_user_id` straight into
composition — `pipeline/mod.rs:373`); identity resolution returns a
`principal_id`. `Identity` keeps ZERO authority fields (D-006)." Kernel-
readiness item 3 (`.raw/openspine-agentos-design-log.md:253`):
"IdentityResolver seam + identity tables (resolve_owner_identity becomes
fast-path)." D-006: identity records store entity knowledge, never authority.

Today the owner is a bare config string threaded straight into the task grant.
That conflates the channel auth credential with the authority-composition
identity, leaves no place to record counterparties, and gives the agent no
honest seam to resolve against. The principal-shaped schema makes promotion to
multi-principal additive later rather than a rewrite.

## Affected layer

OpenSpine core: the `openspine-schemas` crate (`Principal`, `IdentityResolution`)
and the `openspine-kernel` crate (identity store, `IdentityResolver`, the
pipeline identify/compose stages, owner bootstrap). `openspine-authority`
(`AuthorityInput`/`compose_authority`) gets the `principal_id` input. No grant
chain, gate, or audit semantics change; `TaskGrant.user` is unchanged in shape
(now carries the principal id).

## Authority sensitivity

Authority-sensitive. Three load-bearing invariants: (1) D-006 — neither
`Identity` nor `Principal` may carry authority fields; (2) AD-146 — a
counterparty, however richly bound, is not a principal; only the owner fast
path yields a v1 principal; (3) binding is owner-approved and audited, never
agent-triggered. Composition narrowing is strictly safe: it now fails closed
absent a resolved principal where it previously read a config string.

## Goals

- Composition consumes `principal_id`, not `owner_user_id`; a resolved
  principal is the only thing that flows into `TaskGrant.user`.
- Exactly one owner principal, enforced by the database, with a test that a
  second owner insert is rejected.
- The resolver is read-only; unknowns never bind; counterparties resolve to an
  identity but never a principal.
- Binding happens only through the audited owner-principal-context path;
  nothing in any capability pack or the shell handler registry can reach it.
- `Identity`/`Principal` have zero authority fields, asserted by test.

## Non-goals

- No relationship-scoped disclosure policy / `DisclosurePolicy` artifact
  (AD-146 names it; it is a separate future change).
- No persona binding (D-017 — separate).
- No multi-principal support beyond the single-owner v1 schema (the seam is
  additive; promotion is a later change).
- No verification of counterparty identity beyond owner assertion (AD-146 OQ-3
  resolution defers stronger proof).
- No change to grant chain MAC fields, gate semantics, or the audit chain.

## Decision-log check

This change preserves D-004 (binding is an audited owner-control effect, not a
gated agent action — it is a kernel-internal store mutation like
`insert_task_grant`, not a shell effect), D-005/D-010, D-006, D-007 (the
principal is identity-shaped; the task grant remains the only live authority),
and D-008 (identity/principal resolution is deterministic, never agentic). If
implementation reveals a need to weaken or materially refine an accepted
decision, the decision log is updated before completing the change.
