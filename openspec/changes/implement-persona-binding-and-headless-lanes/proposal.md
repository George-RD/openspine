# Persona binding and headless hook lanes

## Dependencies

- `implement-identity-store-and-principal` (landed identity resolution and owner principal).
- `implement-connector-reality` (landed signed, replay-protected webhook authenticity substrate).
- Existing persona overlay machinery from D-094..D-097.

## Problem/Context

AD-136 requires persona fronting to be a deterministic kernel route result from connector/number, sender identity, and relationship. AD-134 requires a verified hook to be an ordinary event source through verification, identification, routing, grants, and gating, with no owner conversation when composed authority needs no approval. The current route schema cannot name the selected persona or constrain a channel account, and webhook verification has no event-pipeline consumer.

## Proposed Solution

Extend route matching additively with an optional channel-account selector and persona artifact reference. Add a pure kernel resolver that returns the route-selected persona only after deterministic route matching; agent code receives no persona-selection input. Add a kernel-owned headless hook entry point that verifies a signed webhook, builds a normalized webhook envelope, resolves identity and route, composes a grant, invokes the existing mediation/gate boundary, and records owner-digest-only surfacing for successful no-approval flows.

## Acceptance Criteria

- Owner traffic to every bound number selects the owner-facing persona; a counterparty on that number cannot select that persona.
- A verified hook completes verification → identification → routing → grant → gate with zero owner conversation when no approval is composed, and emits only an owner digest item.
- Invalid or replayed hooks fail closed before route/grant/effect stages.

## Out of Scope

- Persona authority, proposal, approval, activation, or prompt guidance changes.
- New connector transports beyond the existing webhook verifier.
- Choosing a fixed surfacing volume; that remains a learned overlay preference.
## Design note on binding ownership

The persona binding decision is kernel machinery, not an overlay-authored
`Route` field that an overlay edit could change. This change adds an
additive `persona` reference to the route artifact so the *selected*
persona travels with the deterministic match result, but the binding
table (connector/account number × resolved identity × relationship →
persona) is computed kernel-side from the resolved route, identity,
and relationship — never from agent-supplied or overlay-edited input.
