# Design

## Deterministic disclosure boundary
`DisclosureProvenance` carries only content-addressed item references plus classified disclosure classes. `OutboundQuery::from_private_context` generalizes supplied sensitive terms before transport, and `check_egress` evaluates every provenance class against active policy rows; sensitivity classification never depends on the generalized text, while scoped carve-outs intentionally hash it for `covers()` matching.

The rated egress class is resolved from the trusted `ActionCatalog`, not request metadata. Private, internal, and sensitive classes require a matching active `(RelationshipKind, DisclosureClass)` policy; public context does not.

## Kernel-prepared, digest-bound egress
Every rated egress effect MUST be prepared by the kernel before dispatch: `prepare_disclosure_query` generalizes the raw query from all private/sensitive sections in the grant briefcase, binds it to the action/relationship/egress/grant/provenance, computes a one-way digest, and persists a one-use token. The dispatch hook requires that token, consumes it, and verifies the digest and every binding before the connector sees any text. A missing, consumed, replayed, or mismatched token fails closed with zero connector calls.

## Lazy owner acquisition
An uncovered class returns an owner-only question, creates a durable pending question containing the kernel-derived blocked-query digest, and is routed as `EscalationPayload::OwnerQuestion`; the counterparty receives no policy detail. The owner answers via `/disclosure allow <id>`, `/disclosure allow-with-carve-out <id>`, or `/disclosure deny <id>`. The answer consumer looks up the pending question by id, uses its stored digest for scoped carve-outs, records the scoped policy, and clears the pending question without broadening unrelated approvals.

## Per-scope envelopes and reactivation
Each egress class bound by a policy owns its own standing-rule envelope keyed by the complete `(relationship, disclosure_class, egress_class)` scope. The synthetic action identity and rule id are scope-specific, so revoking or letting one scope lapse cannot silently affect a sibling scope or the real rated egress action. Re-answering a lapsed or revoked envelope bumps its version so reactivation is not a no-op.

## Storage and recovery
Policy JSON is stored in an idempotent SQLite table. `prepared_queries` stores one-use grant/provenance-bound generalized-query tokens; `disclosure_pending_questions` stores durable owner questions and their blocked-query digests. Enforcement loads policy rows from the store on every call and fails closed on read or deserialization failure. The disclosure envelope consults and reserves its D-107 quota/rate budget atomically, finalizing only after successful dispatch and cancelling on pre-effect failure.

## Module boundaries
- `openspine-schemas::disclosure_policy`: strict policy, provenance, query, prepared-query, and pure gate types.
- `openspine-kernel::store::disclosure_policies`: durable policy rows, prepared-query tokens, and pending questions.
- `openspine-kernel::disclosure`: trusted egress resolution, recursive redaction, query preparation, owner-answer persistence, budget reservation, and escalation routing.
- `openspine-kernel::pipeline`: verified owner-answer consumer for disclosure questions.
- `openspine-schemas::escalation` plus `openspine-kernel::escalation`: typed owner-question delivery and audit routing.
