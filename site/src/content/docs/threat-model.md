---
title: Threat model
description: What OpenSpine claims to defend against, what it doesn't, and the tests that prove each claim.
---

## Claims vs exclusions, honestly

A security claim nobody can falsify is marketing, not engineering. Every
claim below maps to a named `cargo test` — or, where a claim genuinely
isn't assertable inside a test (a Docker network topology fact, say), to a
documented `manual:` justification instead of a stretched mapping. The
source of truth is
[`docs/threat-claims.md`](https://github.com/George-RD/openspine/blob/main/docs/threat-claims.md)
in the repository; `scripts/check-claims.sh` fails the build if a claimed
test doesn't exist, or if the register is ever gutted to zero rows.

## Claims

| Claim | Verification |
| --- | --- |
| Telegram owner messages are verified against the configured owner ID before owner routing | `configured_owner_text_message_is_verified` |
| Identity is not authority: a spoofed owner ID without a verified source is denied | `spoofed_owner_id_without_verified_source_is_denied` |
| Connector authentication and account role grant no trust by themselves | `gmail_connector_authenticated_alone_does_not_match_the_selected_thread_route` |
| External communication and content are treated as data, never instruction | `email_reply_drafter_template_wraps_untrusted_context_on_the_wire` |
| The shell receives no raw connector credentials — only `KERNEL_ENDPOINT` and `TASK_TOKEN` | `process_driver_clears_env_and_sets_only_two_vars` |
| The shell cannot directly call arbitrary external APIs in normal operation | manual: network egress containment is a Docker network property (`compose.yaml`'s `openspine-internal` network is `internal: true`) |
| Private-context model calls are mediated by the model gateway, with untrusted context sent wrapped, never raw | `generate_sends_untrusted_context_in_body` |
| User-selected targets are proven with selection tokens bound to the requesting grant | `email_read_selected_thread_rejects_foreign_grant` |
| Selection tokens are single-use | `email_read_selected_thread_rejects_second_use` |
| Authority is composed by deterministic intersection — no candidate allow means the action is not granted | `no_candidate_allow_means_action_is_not_granted` |
| Explicit deny wins over any allow | `explicit_deny_overrides_allow` |
| Approval-required overrides a plain allow | `approval_required_overrides_plain_allow` |
| Every effectful action is mediated by `gate()` before dispatch | `approval_required_action_stops_before_dispatch` |
| Audit records reference encrypted artifact refs for private payloads, never plaintext | `audit_metadata_records_action_grant_and_refs_not_plaintext` |
| The shell cannot widen its own authority without explicit owner approval | `widening_via_a_proposed_pack_requires_approval_first` |
| LLMs may not resolve authority-affecting route conflicts | `priority_tie_with_equal_specificity_is_ambiguous` |
| Final email send is denied regardless of grant or approval state | `global_policy_round_trips_and_denies_send` |
| Kernel replies are channel-bound: always sent to the grant-bound owner chat, never an override | `lyra_ui_preview_sends_telegram_reply_to_grant_bound_chat` |
| System-operations actions (host filesystem, raw network egress) are high-impact and denied by default | `host_filesystem_read_and_write_are_denied_for_owner_control_grant` |

## What the current phases do not claim to defend against

- A malicious root user on the host.
- A compromised kernel process or a compromised host OS.
- A model provider retaining data despite its stated policy.
- A user manually copying private data elsewhere after the kernel has
  legitimately shown it to them.
- Physical device compromise.
- All side-channel leakage.

These claims and exclusions refer to the OpenSpine/Lyra runtime substrate
as a whole, not only the Lyra personal-assistant product.
