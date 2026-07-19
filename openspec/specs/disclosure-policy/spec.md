# disclosure-policy Specification

## Purpose
TBD - created by archiving change implement-disclosure-policy. Update Purpose after archive.
## Requirements
### Requirement: Disclosure coverage is deterministic and provenance-based
The kernel MUST classify briefcase provenance with a disclosure class and MUST evaluate every non-public class against an active policy keyed by concrete relationship kind and disclosure class. The check MUST NOT infer sensitivity from generalized query text.

#### Scenario: Private context is an effect and generalized before egress
- **WHEN** an outbound query is built from classified private context
- **THEN** it MUST be marked as an effect and sensitive terms MUST be generalized before transport
- **AND** `private_context_query_is_an_effect_and_generalized_before_egress` MUST pass

#### Scenario: Generalized public-looking text retains private provenance
- **WHEN** generalized query text no longer contains the sensitive term
- **THEN** the immutable provenance class MUST still require policy coverage
- **AND** `coverage_uses_provenance_even_when_generalized_text_is_public` MUST pass

#### Scenario: Uncovered disclosure class blocks deterministically
- **WHEN** an egress carries sensitive classified provenance without a covering policy
- **THEN** the egress MUST block and produce an owner-question escalation
- **AND** `uncovered_disclosure_class_blocks_and_produces_owner_question_escalation` MUST pass
- **AND** `uncovered_egress_blocks_and_produces_owner_question` MUST pass

#### Scenario: Nested private payload is generalized
- **WHEN** a private section contains sensitive strings below nested objects or arrays
- **THEN** every nested string MUST be considered for redaction before transport
- **AND** `nested_json_sensitive_term_extraction_redacts_all_strings` MUST pass

### Requirement: Owner answers persist scoped policy and carve-outs
The kernel MUST persist an accepted owner answer as a relationship/class policy containing carve-outs and MUST bind each egress envelope to the complete relationship/class/egress scope. Enforcement MUST load persisted policies and fail closed on store errors.

#### Scenario: Owner answer is recoverable
- **WHEN** an owner answer is recorded with a carve-out
- **THEN** a fresh policy read MUST recover the carve-out and the standing-rule envelope MUST be live
- **AND** `owner_answer_persists_as_standing_rule_with_carve_outs` MUST pass
- **AND** `disclosure_policy_recovers_carve_outs_from_store` MUST pass

#### Scenario: Two scopes have independent envelopes
- **WHEN** two relationship/class scopes are approved for the same egress class
- **THEN** each scope MUST have a distinct envelope and revoking one MUST leave its sibling live
- **AND** `per_scope_envelope_revocation_leaves_sibling_live` MUST pass

#### Scenario: Same scope gains another egress class
- **WHEN** a relationship/class receives approvals for two egress classes
- **THEN** both egress approvals and carve-outs MUST remain effective
- **AND** `same_scope_two_egress_classes_merge_without_erasing` MUST pass

#### Scenario: Disclosure envelope cannot revoke normal egress authority
- **WHEN** a normal standing rule already authorizes a rated egress action
- **THEN** recording a disclosure answer MUST NOT revoke that normal rule
- **AND** `disclosure_policy_does_not_revoke_existing_egress_standing_rule` MUST pass

### Requirement: Owner questions use the canonical escalation surface
An uncovered disclosure block MUST be representable as a typed owner-question escalation and MUST remain separate from counterparty-facing refusal text.

#### Scenario: Owner-question payload is deliverable
- **WHEN** the deterministic disclosure check blocks
- **THEN** the resulting escalation MUST use `EscalationPayload::OwnerQuestion`
- **AND** `owner_question_escalation_is_deliverable_on_owner_channel` MUST pass

### Requirement: Rated egress requires a kernel-prepared, digest-bound query
Before any rated egress effect, the kernel MUST mint a generalized query token bound to the action, relationship, egress class, requesting grant, and kernel-derived provenance. Dispatch MUST consume and verify that token before the connector sees any text. A missing, consumed, replayed-under-another-grant, or mismatched token MUST fail closed with zero connector calls.

#### Scenario: Prepared query mints, verifies digest, and is one-use
- **WHEN** the kernel prepares a rated egress query from classified provenance
- **THEN** it MUST persist a grant/provenance-bound digest token that dispatch can consume exactly once
- **AND** a second consume, a different grant, and a tampered digest reference MUST all be rejected
- **AND** `prepared_query_mints_consumes_once_and_verifies_digest` MUST pass
- **AND** `binding_matches_rejects_a_different_requesting_grant` MUST pass

### Requirement: D-107 disclosure budgets are consulted atomically
The disclosure envelope MUST consult and reserve its own D-107 quota and rate windows before allowing an egress. Exhaustion MUST block the disclosure without consuming a connector budget reservation.

#### Scenario: Exhausted disclosure budget blocks
- **WHEN** the scoped D-107 quota or rate window has no headroom
- **THEN** the disclosure gate MUST block even if the envelope lifecycle is active
- **AND** `disclosure_envelope_budget_exhaustion_blocks` MUST pass

### Requirement: Per-egress envelopes are independent and reactivate on re-answer
Each egress class bound by a policy MUST own its own scope-specific standing-rule envelope; revoking or letting one egress lapse MUST NOT silently keep another egress authorized through the same policy row. Re-answering a lapsed or revoked envelope MUST bump the envelope version so reactivation is not a no-op.

#### Scenario: Revoking one egress leaves the other authorized
- **WHEN** a scope is approved for two egress classes and one envelope is revoked
- **THEN** only the revoked egress MUST block while the other remains allowed
- **AND** `per_egress_revocation_is_independent` MUST pass

#### Scenario: Re-answer after revoke reactivates via version bump
- **WHEN** an approved egress envelope is revoked and the owner answers again
- **THEN** the envelope version MUST increase and enforcement MUST allow the egress again
- **AND** `reanswer_after_revoke_reactivates_via_version_bump` MUST pass

### Requirement: Owner answers resolve durable pending questions
An uncovered disclosure block MUST create a durable pending question containing the blocked-query digest. The owner MUST answer by pending-question id; broad allow, scoped allow, and deny MUST each have distinct behavior. A scoped answer MUST use the stored digest and the answer consumer MUST clear the question without accepting a human-supplied digest.

#### Scenario: Scoped owner answer covers the blocked query
- **WHEN** the owner answers an open disclosure question with allow-with-carve-out and its pending id
- **THEN** the resulting carve-out MUST cover the stored blocked-query digest
- **AND** `scoped_owner_answer_uses_pending_question_digest` MUST pass

#### Scenario: Owner disclosure answer creates policy and clears pending
- **WHEN** the owner answers an open disclosure question with allow
- **THEN** a relationship/class policy authorizing that egress MUST be recorded and the pending question MUST be cleared
- **AND** `owner_disclosure_answer_creates_policy_and_clears_pending` MUST pass

### Requirement: Disclosure failures are generic to workers and preserve store errors
Kernel store errors from owner-answer recording MUST propagate to the pipeline. Worker-facing disclosure denials MUST use generic stable messages and MUST NOT expose relationship, class, question, or debug-formatted policy details.

#### Scenario: Worker receives generic disclosure denial
- **WHEN** a prepared-query binding, policy, or owner-answer store operation fails
- **THEN** the worker-facing response MUST use a generic stable denial code and the underlying store error MUST remain an error for kernel handling
- **AND** `worker_denial_has_no_debug_leak` MUST pass

