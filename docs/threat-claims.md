# Threat claims register

Source: `.raw/openspine-prd-v9.md` §25 ("Threat-model exclusions" / "Phase 1
and Phase 2 do claim") plus the core invariants in §1. Each row is a
security claim this repository makes; `Verification` is either a `cargo
test` name that enforces the claim, or a `manual:` justification for a
claim that isn't assertable inside `cargo test` (e.g. a property of the
Docker network topology). `scripts/check-claims.sh` (wired into
`scripts/check.sh`) fails the build if any `test:` name here doesn't
exist in the workspace.

| ID | Claim | Verification |
| --- | --- | --- |
| CLAIM-01 | Telegram owner messages are verified against the configured owner ID before owner routing | `test: configured_owner_text_message_is_verified` |
| CLAIM-02 | Identity is not authority: a spoofed owner ID without a verified source is denied | `test: spoofed_owner_id_without_verified_source_is_denied` |
| CLAIM-03 | Connector authentication and account role grant no trust by themselves | `test: gmail_connector_authenticated_alone_does_not_match_the_selected_thread_route` |
| CLAIM-04 | External communication and content are treated as data, never instruction | `test: email_reply_drafter_template_wraps_untrusted_context_on_the_wire` |
| CLAIM-05 | The shell receives no raw connector credentials — only `KERNEL_ENDPOINT` and `TASK_TOKEN` | `test: process_driver_clears_env_and_sets_only_two_vars` |
| CLAIM-06 | The shell cannot directly call arbitrary external APIs in normal operation | `manual: network egress containment is a Docker network property (compose.yaml's openspine-internal network is internal: true); not assertable inside cargo test` |
| CLAIM-07 | Private-context model calls are mediated by the model gateway, with untrusted context sent wrapped, never raw | `test: generate_sends_untrusted_context_in_body` |
| CLAIM-08 | User-selected targets are proven with selection tokens bound to the requesting grant | `test: email_read_selected_thread_rejects_foreign_grant` |
| CLAIM-09 | Selection tokens are single-use | `test: email_read_selected_thread_rejects_second_use` |
| CLAIM-10 | Authority is composed by deterministic intersection — no candidate allow means the action is not granted | `test: no_candidate_allow_means_action_is_not_granted` |
| CLAIM-11 | Explicit deny wins over any allow | `test: explicit_deny_overrides_allow` |
| CLAIM-12 | Approval-required overrides a plain allow | `test: approval_required_overrides_plain_allow` |
| CLAIM-13 | Every effectful action is mediated by `gate()` before dispatch | `test: approval_required_action_stops_before_dispatch` |
| CLAIM-14 | Audit records reference encrypted artifact refs for private payloads, never plaintext | `test: audit_metadata_records_action_grant_and_refs_not_plaintext` |
| CLAIM-15 | The shell cannot widen its own authority without explicit owner approval | `test: widening_via_a_proposed_pack_requires_approval_first` |
| CLAIM-16 | LLMs may not resolve authority-affecting route conflicts | `test: priority_tie_with_equal_specificity_is_ambiguous` |
| CLAIM-17 | Final email send is denied regardless of grant or approval state | `test: global_policy_round_trips_and_denies_send` |
| CLAIM-18 | Kernel replies are channel-bound: always sent to the grant-bound owner chat, never an override | `test: lyra_ui_preview_sends_telegram_reply_to_grant_bound_chat` |
| CLAIM-19 | System-operations actions (host filesystem, raw network egress) are high-impact and denied by default, not casually allowed | `test: host_filesystem_read_and_write_are_denied_for_owner_control_grant` |
 | CLAIM-20 | Kernel-origin effects are routed through `gate()` and are audit-never-exempt — `notify_owner_best_effort` is approval-exempt but still emits `AuditMeta`, never bypassing the gate or its audit | `test: owner_notify_routes_through_gate_and_audits` |
 | CLAIM-21 | A kernel-origin call for an action outside the enumerated trusted-origin set is denied by `gate()` | `test: kernel_origin_call_outside_trusted_set_is_denied` |
 | CLAIM-22 | A kernel-origin call for an enumerated trusted action (`owner.notify`) is auto-allowed (approval-exempt) yet always emits `AuditMeta` — never audit-exempt | `test: kernel_origin_owner_notify_is_auto_allowed` |
 | CLAIM-23 | Selection-token validation (bound grant, missing/expired/foreign/wrong-type) is a `gate()` decision property, not a dispatch-site check | `test: token_requiring_action_denied_for_foreign_grant` |
 | CLAIM-24 | The kernel re-derives the payload digest at approval-effect time and denies the effect if the stored payload was mutated since approval; no shell-supplied digest is trusted | `test: payload_mutated_since_approval_is_denied_and_creates_no_draft` |
 | CLAIM-25 | `answerCallbackQuery` is a control-plane ack with no security effect and is classified as a non-effect path (no `gate()` authority) | `test: answer_callback_query_is_a_control_plane_ack_with_no_security_effect` |
| CLAIM-26 | Authenticated Macaroons-simple grant chains reject authority, identity, selection-token, caveat-order, and lineage tampering | `test: id_parent_and_action_list_tamper_fail` `test: selection_token_tamper_invalidates_mac` `test: caveat_reorder_or_remove_invalidates_mac` `test: identity_field_tamper_invalidates_mac` |
| CLAIM-27 | Shadow grants are represented as a non-executable gate decision and do not run effects | `test: shadow_allow_is_non_executable_effect_suppressed` `test: shadow_grant_effect_suppressed_skips_effect_handler` |
| CLAIM-28 | AD-036 bound-parameter caveats cannot conflict or be widened across a chain | `test: bound_parameter_conflict_is_caveat_widening` |
| CLAIM-29 | Identity store enforces at most one owner principal at the database layer | `test: database_enforces_at_most_one_owner_principal` |
| CLAIM-30 | Identity bootstrap/initialization is idempotent and fails closed if DB owner mismatch occurs | `test: bootstrap_owner_principal_creates_exactly_one_owner_and_is_idempotent` `test: bootstrap_owner_principal_fails_closed_on_config_mismatch` |
| CLAIM-31 | Counterparty identity binding is gated on an authenticated owner-principal context at the API boundary, and is audited atomically; unknown claims never auto-bind | `test: owner_assert_binding_succeeds_and_is_audited_atomically` `test: owner_assert_binding_rejects_non_owner_principal_id` `test: unknown_resolves_to_relationship_unknown_confidence_0_and_no_write` |
| CLAIM-32 | A skill is an instruction surface that shapes competence only: it carries no authority-shaped field (deny_unknown_fields), and a poisoned skill body cannot widen granted actions — `gate()` mediates every action regardless of which surface suggested it (AD-040). The causal path (malicious installed skill -> real skill.context dispatch -> derived action attempt) is tested end-to-end. | `test: skill_wire_shape_rejects_authority_fields` `test: containment_gate_denies_denied_action_regardless_of_malicious_skill` `test: causal_containment_through_skill_context_dispatch` |
| CLAIM-33 | A mined skill cannot reach the shelf without a passing, digest-bound AD-110 promotion review, and the review verdict (approved/rejected) is persisted in the eval-verdict store and queryable before the skill is promoted (audit-before-effect) | `test: promotion_review_approved_verdict_is_queryable` `test: promotion_review_denial_verdict_is_queryable` `test: promotion_review_rejects_body_digest_mismatch` `test: verdict_recorded_before_promotion_effect_is_atomic` |
| CLAIM-34 | The owner promotion tap is the sole owner entry point that lands a mined skill and authenticates with a `VerifiedOwnerContext` + owner-principal binding; owner approval routes through the AD-110 evaluator (necessary but not sufficient to bypass it) and the decision is durably persisted atomic with the shelf transition | `test: owner_approval_promotes_through_evaluator` `test: owner_decide_rejects_unknown_principal` `test: owner_rejection_keeps_skill_off_shelf` |
| CLAIM-35 | AD-041 "one decision per skill version, ever": a repeat owner tap on an already-decided skill id+version fails closed and persists no contradictory second row; an owner-approve-that-the-evaluator-denies is recorded with decision `approve` (owner intent) and `result_state=Rejected`, never mislabeled as an owner rejection | `test: repeat_owner_tap_on_same_version_fails_closed` `test: owner_approve_but_evaluator_denies_labels_decision_approve` |
| CLAIM-36 | Installing or promoting a higher skill version atomically retires every lower `Installed` version of the same id (same `Retired` terminal state as an owner withdrawal); the shelf exposes at most the highest active version per id, and a downgrade/equal version is refused | `test: promotion_retires_lower_installed_version_atomically` `test: trusted_install_refuses_lower_version_when_higher_exists_any_state` `test: promotion_refuses_when_higher_installed_exists` |
| CLAIM-37 | `skill.context` is a real gated kernel action: it derives the agent/pack scope from the authenticated `TaskGrant` (never the caller's payload) and returns skill `body` text only inside an explicit `untrusted: true` envelope, so a grantee can query only its grant-scoped shelf and the returned competence data confers no authority | `test: skill_context_selects_only_grant_scoped_installed_matches` |

- CLAIM-05 is enforced by `sandbox::tests::process_driver_clears_env_and_sets_only_two_vars`,
  which spawns a real child process and inspects its actual environment
  (not just the constructed argument vector) — a stronger check than the
  originally-planned "assert on `Command` args" version, and it already
  existed before this register was written; the equivalent Docker-side
  guarantee is `sandbox::tests::docker_driver_args_are_correct_and_secret_free`.
- CLAIM-06 has no `test:` mapping because the property it asserts
  (no route from the shell's network namespace to the public internet)
  is a `compose.yaml` topology fact, not something `cargo test` can
  observe.
