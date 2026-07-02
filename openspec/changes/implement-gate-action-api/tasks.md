# Tasks: Implement gate action API

## 1. Types

- [x] Define action request type. (already defined in `define-core-runtime-schemas`; extended here with `target_digest: Option<Digest>` — needed for approval-digest matching but action-specific, so not a generic `TargetRef`.)
- [x] Define gate decision type. (`openspine_schemas::action::GateDecision`, defined in Step 1; reused rather than duplicated per that module's doc comment.)
- [x] Define denial reason enum or equivalent. (`openspine_schemas::action::DenialReason`, Step 1.)
- [x] Define approval-required decision shape. (`GateDecision::ApprovalRequired { approval_type }`.)
- [x] Define audit metadata shape. (`openspine_gate::AuditMeta`.)

## 2. Gate implementation

- [x] Implement denied-action check.
- [x] Implement approval-required check. (Distinguishes "never asked" → `ApprovalRequired` from "a decision already exists but doesn't currently authorize this exact payload/target" → `Deny` — D-011: an edited-after-approval payload must never fall back to a re-ask.)
- [x] Implement allowed-action check.
- [x] Implement unspecified deny.
- [x] Return structured decision. (`GateOutcome { decision, audit }`.)

## 3. Audit

- [x] Emit or return audit metadata for every decision. (`gate()` returns `AuditMeta` alongside every `GateDecision`; it does not write the audit row itself — no I/O in this crate.)
- [x] Ensure private payloads are refs/hashes only. (`AuditMeta` carries `ArtifactRef`/`Digest`/`TargetRef`, never plaintext — enforced by the type system, not a runtime check.)
- [x] Add denial audit examples. (`denial_audit_metadata_still_carries_refs` test.)

## 4. Tests

- [x] Test allowed action returns allow.
- [x] Test denied action returns deny.
- [x] Test approval-required action returns approval-required.
- [x] Test allowed plus denied returns deny.
- [x] Test allowed plus approval-required returns approval-required.
- [x] Test unspecified action returns deny.
- [x] Test approval-required action does not execute. (Framed as: the outcome is never `Allow` without a matching approval.)
- [x] Test expired grant denies even an allowed action (not in the original checklist; added because `is_expired` gates every other check per design.md).
- [x] Test digest-bound approval regression matrix for Step 6 (not in the original checklist; added because Step 6's plan assumes `gate()` already distinguishes matching/mismatched/expired/rejected approvals — `openspine-gate` is not touched again in Step 6): matching approval allows; payload changed after approval denies (`ApprovalDigestMismatch`), not re-asks; expired approval denies (`ApprovalExpired`); rejected approval denies (`ApprovalMissing`).

## 5. Validation

- [x] Run unit tests (14 tests in `openspine-gate`; 20 unaffected in `openspine-authority`, 57 unaffected in `openspine-schemas`).
- [x] Run `openspec validate --changes implement-gate-action-api --strict` — verified green.
