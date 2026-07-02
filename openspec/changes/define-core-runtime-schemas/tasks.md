# Tasks: Define core runtime schemas

## 1. Create schema location

- [x] Decide the initial schema location: `crates/openspine-schemas/src/` (one Rust module per schema group), declarative product artifacts under `artifacts/lyra/**/*.yaml`.
- [x] Add a README explaining that schemas define runtime objects but do not activate authority (`artifacts/README.md`).

## 2. Define event schemas

- [x] Define `event_envelope` (`crates/openspine-schemas/src/event.rs::EventEnvelope`).
- [x] Define source verification fields (`verified_source`, `verification_method`).
- [x] Define replay protection fields (`replay_protected`, `replay_nonce`).
- [x] Define actor hints and target refs (`ActorHint`, `TargetRef`).
- [x] Define lane and trust context fields (`Lane`, `TrustContext`).

## 3. Define identity and route schemas

- [x] Define `identity_record` (`identity.rs::Identity`).
- [x] Define `identity_resolution` (`identity.rs::IdentityResolution`).
- [x] Define `route_artifact` (`route.rs::Route`).
- [x] Define `route_resolution` (`route.rs::RouteResolution`).

## 4. Define authority schemas

- [ ] Define `authority_composition_input` — **deferred to `implement-authority-composition`**: the build plan (Step 2) defines this as `openspine_authority::AuthorityInput<'a>`, a borrowed-reference bundle over this crate's types, not a persisted/serializable schema object. Design.md's grouping is looser than the approved implementation plan; recorded here rather than silently ticked.
- [ ] Define `authority_composition_result` — same deferral: `openspine_authority::AuthorityOutcome` (Step 2).
- [x] Define `task_grant` (`grant.rs::TaskGrant`).
- [x] Include allowed, denied, and approval-required actions (`TaskGrant::{allowed_actions,denied_actions,approval_required_actions}`).

## 5. Define action/approval/model/audit schemas

- [x] Define `action_request` (`action.rs::ActionRequest`).
- [x] Define `gate_decision` (`action.rs::GateDecision`, `DenialReason`).
- [x] Define `approval_record` (`approval.rs::ApprovalRecord`).
- [x] Define `selection_token` (`selection.rs::SelectionToken`).
- [x] Define `model_request` (`model.rs::ModelRequest`).
- [x] Define `audit_event` (`audit.rs::AuditEvent`).
- [x] Define `artifact_ref` (`artifact.rs::ArtifactRef`).

## 6. Verification

- [x] Add examples for Telegram owner message and selected-thread email event (`artifacts/lyra/routes/*.yaml`; envelope construction covered by `event.rs` unit tests).
- [x] Add examples for owner-control task grant and selected-thread email task grant (`grant.rs` unit tests use the PRD §12.1/§12.2 field values).
- [x] Validate that identity records do not include live capability grants (`identity.rs::identity_json_has_no_authority_field`, `deny_unknown_fields_rejects_capability_pack_id`).
- [x] Validate that route artifacts do not directly grant final authority (routes only carry candidate `agent`/`workflow`/`capability_pack` refs; authority is materialized only by `openspine-authority` in Step 2).
- [x] Validate that approval records bind payload and target digests (`approval.rs::matches_requires_both_digests_and_approved_decision`).
- [x] Validate that audit examples do not store private payloads as raw text (`audit.rs::rejects_unknown_fields`, `target_and_payload_refs_default_to_empty_when_omitted`).

## 7. Review

- [x] Confirm consistency with `.raw/openspine-prd-v9.md`.
- [x] Confirm consistency with `.raw/openspine-decision-log.md`.
- [x] Run `scripts/check.sh define-core-runtime-schemas` (fmt, clippy -D warnings, workspace tests incl. `openspec validate define-core-runtime-schemas --strict`, file-size gate) — verified green.
