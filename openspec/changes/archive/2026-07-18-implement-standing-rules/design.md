# Design

## Artifact and ceremony
`StandingRuleManifest` is a strict versioned artifact containing one action id, independent quota and rate windows, lapse-on-unused expiry, and an optional dark-window default. It enters runtime only through the generic proposed → evaluated → owner-approved → activated artifact path. Activation writes the rule row and audit evidence transactionally; revocation removes it from live lookup.

A standing rule is an input to mediation. It never replaces or mutates the authenticated task grant. The normal gate runs first. Only an `ApprovalRequired` result may consult a matching active rule.

## Atomic budget consultation
The store opens a `BEGIN IMMEDIATE` transaction, loads an active rule, checks independent trailing quota and rate windows, records successful consumption, updates last use, and returns remaining headroom. Saturation returns false without recording usage. Concurrent callers therefore cannot overspend a boundary. Expired, revoked, drifted, and unknown rules return no live budget.

Repeated velocity saturation moves the rule to `needs_review` and appends audit evidence before commit. It then disappears from live consultation until the owner re-confirms through the normal ceremony.

## Dark windows
An over-budget rule with an AD-012 conditional default schedules a durable timer keyed by rule, version, and stable request fingerprint. A fired allow default creates one digest-bound, one-use token that must re-enter the normal gate and can satisfy only `ApprovalRequired`; deny creates none. Timer application is idempotent by timer id, owner Allow/Deny resolution is first-write-wins, and recovery replays only undecided dispatches. Payloads remain encrypted `ArtifactRef` values; malformed persisted identities fail closed.

## Module boundaries
- `openspine-schemas::standing_rule`: strict wire shape.
- `store::standing_rules` and `standing_rules_budget`: lifecycle and atomic counters.
- `standing_rules_gate`: mediation adapter.
- `pipeline::standing_rule_timer`: durable timer consumer.
- `api::actions`: ordinary gate first, then rule consultation.

The bounded Gmail read handler is extracted into `api::read_thread`; scheduling regressions are split into a dedicated test module to preserve the 500-line source gate.
