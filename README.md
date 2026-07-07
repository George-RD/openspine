# OpenSpine

OpenSpine is a safety layer for AI agents. It decides what your agent is **allowed** to do, and proves it.

[![Support on Ko-fi](https://img.shields.io/badge/Ko--fi-Support-ff5e5b?logo=ko-fi&logoColor=white)](https://ko-fi.com/george_builds)

## What is this?

AI agents today can do too much. They often access files or run commands without permission. OpenSpine sits under the agent to check every action. Everything is off until you turn it on (deny-by-default). Every decision is written to a tamper-evident log (hash-chain audited). Lyra is the first app built on OpenSpine. It is a Telegram assistant that drafts Gmail replies but can never send them.

## Why it's different

Other tools focus on what an AI agent can do. They add more tools, more connectors, and more freedom. This leads to security failures. A bad prompt can make the agent take actions you did not want.

OpenSpine puts safety first. The base layer (substrate) owns the rules, not the AI model. An agent has no trust by default. It can only do what you explicitly allow.

To keep things safe, OpenSpine does three things on purpose:
* We do not have a store to download new tools.
* The agent cannot earn more trust over time.
* The agent is blocked from sending emails. Lyra can only draft emails, never send them.

## Try it in 5 minutes

```sh
git clone https://github.com/George-RD/openspine.git
cd openspine
npm ci # dev tools used by the check script
cargo build --workspace
./scripts/check.sh # runs every test and check - same as CI
```

To run a real server, you need to set up some secrets as environment variables:
* `OPENSPINE_TELEGRAM_BOT_TOKEN`: Your Telegram bot token.
* `OPENSPINE_ARTIFACT_KEY`: A random 32-byte hex key. You can make one by running `openssl rand -hex 32`.
* Your model provider API key (like OpenAI or Anthropic).

See the docs below to set up Telegram and Gmail.

## Every claim has a test

We do not just say OpenSpine is safe. Each row below links to a test you can run yourself.

| Claim | Proof |
| --- | --- |
| Telegram owner messages are verified against the configured owner ID | `configured_owner_text_message_is_verified` |
| Identity is not authority: a spoofed owner ID without a verified source is denied | `spoofed_owner_id_without_verified_source_is_denied` |
| Connector authentication and account role grant no trust by themselves | `gmail_connector_authenticated_alone_does_not_match_the_selected_thread_route` |
| External content is data, never instruction | `email_reply_drafter_template_wraps_untrusted_context_on_the_wire` |
| The shell receives no raw connector credentials | `process_driver_clears_env_and_sets_only_two_vars` |
| Private-context model calls are mediated by the model gateway, untrusted context always wrapped | `generate_sends_untrusted_context_in_body` |
| User-selected targets are proven with selection tokens, single-use | `email_read_selected_thread_rejects_foreign_grant`, `email_read_selected_thread_rejects_second_use` |
| Authority composes by deterministic intersection: no candidate allow means no grant | `no_candidate_allow_means_action_is_not_granted` |
| Explicit deny wins over any allow; approval-required overrides a plain allow | `explicit_deny_overrides_allow`, `approval_required_overrides_plain_allow` |
| Every effectful action is mediated by `gate()` before dispatch | `approval_required_action_stops_before_dispatch` |
| Audit records reference encrypted artifacts, never plaintext | `audit_metadata_records_action_grant_and_refs_not_plaintext` |
| The shell cannot widen its own authority without explicit owner approval | `widening_via_a_proposed_pack_requires_approval_first` |
| LLMs may not resolve authority-affecting route conflicts | `priority_tie_with_equal_specificity_is_ambiguous` |
| Email send is denied regardless of grant or approval state | `global_policy_round_trips_and_denies_send` |
| Kernel replies are channel-bound to the grant-bound owner chat | `lyra_ui_preview_sends_telegram_reply_to_grant_bound_chat` |
| System-operations actions (host filesystem, raw network egress) are denied by default | `host_filesystem_read_and_write_are_denied_for_owner_control_grant` |

## Docs

You can read our full documentation at [george-rd.github.io/openspine](https://george-rd.github.io/openspine/).

Below is a map of the documents in this repository:

| Document | What it covers |
| --- | --- |
| [`.raw/openspine-prd-v9.md`](.raw/openspine-prd-v9.md) | The product/architecture spec: envelope shapes, artifact examples, phase exit criteria. |
| [`.raw/openspine-decision-log.md`](.raw/openspine-decision-log.md) | Why the architecture is shaped the way it is (48 decisions, D-001–D-049), and closed open questions (O-001–O-008). |
| [`docs/threat-claims.md`](docs/threat-claims.md) | Every security claim mapped to the test (or documented manual justification) that proves it. |
| [`openspec/`](openspec/) | The OpenSpec-driven development process: 11 applied capability specs, in-flight changes, and the implementation sequence in [`openspec/openspine-change-sequence.md`](openspec/openspine-change-sequence.md). |
| [`openspec/conventions.md`](openspec/conventions.md) | Per-change ceremony: proposal → spec → design → tasks → archive. |

## Status

This project is in Alpha. We have finished the first three phases of our plan. This includes checked actions (gate-mediated action API), Telegram control, Gmail draft previews, and basic rule updates.

We have deferred some work for later. This includes safe secret storage, a thread selector, and per-kind rules.

## License

Free to use. MIT or Apache 2.0 - pick whichever suits you.
