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

## Notes

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
