# Proposal: Implement nerve subscribers

## Dependencies

- `implement-event-bus-subscriptions` (archived): typed filters and durable checkpoints over the audit ledger.
- `implement-egress-classes` (archived): the existing typed egress policy surface; nerves do not bypass it.

## Problem/Context

AD-130 defines nerves as declared, typed event-bus subscribers rather than ad hoc sidecars. A declaration must bind a subscription filter, measure, speak threshold, hard budget, model tier, and data scope no broader than the advisee. AD-051 and AD-112 additionally require advisors to be legibility checkers that emit structured objections, never replacement answers, while cross-scope hints remain structured and gate-visible. AD-052 and AD-132 require bounded proactivity, provenance, reaction feedback, and ignored-class retirement.

The archived event-bus substrate currently provides the filter and checkpoint primitives but no nerve declaration, registration, budget admission, or structured interjection contract.

## Proposed Solution

Add a versioned `openspine-schemas::nerve` module containing the five declared nerve types, model tiers, measures, constrained scope, speak threshold, hard budget, provenance, and structured interjection payloads. Registration compares the nerve scope with its advisee scope and fails closed when the nerve would see broader data.

Add a kernel store module backed by small `CREATE TABLE IF NOT EXISTS` tables for registrations, atomic interjection budget usage, and reaction/ignored-class decay. Registration metadata and decay counters contain no interjection text or private payload. Interjections are admitted only after deterministic threshold, retirement, payload, and registration checks; successful admission consumes one durable budget unit.

## Acceptance Criteria

- A declaration whose scope is broader than its advisee is rejected as unregistrable.
- The five declared nerve types round-trip through the versioned schema.
- Advisor interjections expose concern class, cited clause, suggested rewrite, provenance, and gate visibility without an answer field.
- A threshold-approved interjection consumes one hard budget unit; exhausted budgets and retired classes do not emit.
- Reaction bookkeeping persists and retires a class after five ignored reactions.
- Strict OpenSpec validation and all required Rust/file-size gates pass.

## Out of Scope

- A live broker or push delivery path; replay remains on the archived audit-ledger bus.
- Model invocation, injector/miner policy execution, or direct connector effects.
- Quiet activation of any proposal or authority-bearing artifact.
- Persisting private interjection text in SQLite audit or registration rows.
