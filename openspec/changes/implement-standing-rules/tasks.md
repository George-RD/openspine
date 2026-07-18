## 1. Schema and persistence
- [x] Add the strict standing-rule manifest and artifact-kind parser.
- [x] Add versioned storage, activation, revocation, expiry, and migration coverage.

## 2. Gate-time enforcement
- [x] Consult rules only after ordinary gate approval is required.
- [x] Atomically enforce independent quota and rate windows.
- [x] Return remaining budget in the action response.
- [x] Pull drifted rules from live consultation and surface re-review audit evidence.

## 3. Durable dark windows
- [x] Schedule one durable dark-window timer per rule version and stable request identity (distinct pending timers for distinct stable request identities under the same rule; the uniqueness key is `(rule_id, request_fingerprint)`).
- [x] Apply allow/deny defaults idempotently under replay.

## 4. Ceremony and verification
- [x] Route standing-rule artifacts through proposal, eval-gate, owner approval, and activation.
- [x] Cover expiry, quota, rate, drift, dark-window polarity, exact-deadline boundary, claimed-pending recovery, replay, mediation, and fault paths with real tests: `standing_rule_lapses_after_expiry_unused`, `manifest_validate_rejects_non_positive_windows`, `consult_and_reserve_atomic_budget_saturates_after_max_uses`, `consult_and_reserve_cancel_leaves_headroom_unchanged`, `consult_and_reserve_is_atomic_wrt_version_reactivation`, `owner_revoke_action_removes_rule_from_live_consultation`, `standing_rule_read_failure_cancels_reservation_no_leak`, `standing_rule_rate_window_saturates_independent_of_quota`, `standing_rule_concurrent_final_unit_race_exactly_one_wins`, `standing_rule_drift_saturates_needs_review`, `standing_rule_gate_response_exposes_headroom`, `standing_rule_activation_ceremony_reaches_live_consultation`, `deny_default_never_dispatches_and_is_terminal`, `fired_allow_token_is_digest_bound_and_one_use`, `fired_allow_token_rejects_different_fingerprint`, `scheduling_is_idempotent_across_terminal_resolution`, `owner_resolution_before_fire_controls_claim`, `allowed_pending_is_recoverable_until_consumed`, `claimed_fired_pending_is_surfaced_once_not_redispatched`, `exact_deadline_expiry_boundary_is_uniform`, `reactivated_version_gets_distinct_pending_timer`, `decode_missing_payload_ref_propagates_error`, `recovery_surfaces_claimed_and_propagates_missing_none_payload`, `standing_rule_full_mediate_flow_with_activated_rule`, `standing_rule_effective_allow_audit_failure_cancels_reservation`, `standing_rule_normal_deny_exposes_no_headroom`, `standing_rule_fired_path_audit_failure_rearms_token_once`, `artifact_revoke_dispatch_removes_rule_from_live_consultation`, `standing_rule_timer_redelivery_dispatches_once`.
- [x] Run `./scripts/check.sh implement-standing-rules`.
