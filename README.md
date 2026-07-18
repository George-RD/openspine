# OpenSpine

**The backbone for agents you can actually trust with your life admin.**

OpenSpine is a self-hostable runtime for governed agents. The runtime decides what an agent is allowed to do — never the model, never the prompt. Every decision lands in a tamper-evident audit log, and every documented security claim maps to a named test the build enforces.

[![Support on Ko-fi](https://img.shields.io/badge/Ko--fi-Support-ff5e5b?logo=ko-fi&logoColor=white)](https://ko-fi.com/george_builds)

## Who this is for

**You want an assistant that touches your real life** — your email, your messages — but handing an LLM your Gmail key and a prompt that says "be careful" is not a security model. OpenSpine is the alternative: the agent runs inside a runtime that structurally cannot send your email, read threads you didn't select, or touch your filesystem, no matter what the model decides or what a poisoned email tells it.

**You build agent products** and you're tired of safety being a paragraph in the system prompt. OpenSpine makes authority a runtime property: you declare what an agent may do, the kernel composes and enforces it, and the agent operates inside that boundary. A prompt injection can change what the model *wants*; it cannot change what the runtime *permits*.

## How it works

Every event travels the same spine, in the same order, every time:

```
event → verify → identify → route → compose → grant → run → gate → audit
```

- The source is **verified** before anything else happens. A sender ID is checked, not believed.
- Identity is resolved, but **identity is never authority**. Knowing who you are grants nothing by itself.
- Authority is **composed** by deterministic intersection of routes, agent manifests, workflows, capability packs, and policies. Deny by default: no rule allows it, it doesn't happen.
- The agent receives one **task grant** — short-lived, scoped, budgeted. That grant is the only live authority object in the system.
- Every effect passes through one **gate** before any connector runs it: allow, deny, or ask you first.
- Everything is **audited** into a hash-chained log that references encrypted artifacts, never plaintext.

The agent itself runs in a contained shell with exactly two environment variables and no route to the internet. It never sees a credential. Its only door back into the world is the kernel API — and the gate is in the doorframe.

## Trust grows, but only through you

An agent on OpenSpine can propose new routes, rules, and capabilities. It can never activate them. Every proposal is shown to you exactly as it will run — digest-bound, so what you approve is byte-for-byte what activates — and nothing turns itself on. There is no tool store, no silent capability creep, no "the agent decided it needed more access." Capability grows exactly as fast as your approvals, and no faster.

## Lyra, the first product

Lyra is a personal assistant built on OpenSpine: you talk to it on Telegram (verified against your owner ID), it reads the Gmail threads you select, and it drafts replies for your approval. It cannot send email — not as a setting, but as policy the runtime enforces regardless of grant or approval state. The draft it creates is the draft you approved, verified by digest.

## Every claim has a test

A safety claim nobody can falsify is marketing. Each row links to a test you can run yourself; `scripts/check-claims.sh` fails the build if a claimed test stops existing.

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

## Try it in 5 minutes

```sh
git clone https://github.com/George-RD/openspine.git
cd openspine
npm ci                # dev tools used by the check script
cargo build --workspace
./scripts/check.sh    # runs every test and check - same as CI
```

To run a real server you need three secrets as environment variables:

- `OPENSPINE_TELEGRAM_BOT_TOKEN` — your Telegram bot token.
- `OPENSPINE_ARTIFACT_KEY` — a random 32-byte hex key (`openssl rand -hex 32`). Every private message, email, and prompt is stored encrypted with it.
- Your model provider API key (Anthropic, OpenAI, or compatible).

The [quickstart](https://george-rd.github.io/openspine/quickstart/) walks through Telegram and Gmail setup.

## Docs

Full documentation lives at [george-rd.github.io/openspine](https://george-rd.github.io/openspine/).

Inside the repository:

| Document | What it covers |
| --- | --- |
| [`.raw/openspine-prd-v9.md`](.raw/openspine-prd-v9.md) | The product/architecture spec: envelope shapes, artifact examples, phase exit criteria. |
| [`.raw/openspine-decision-log.md`](.raw/openspine-decision-log.md) | Why the architecture is shaped the way it is — every decision with its rationale, consequences, and the condition that would reverse it. |
| [`docs/threat-claims.md`](docs/threat-claims.md) | Every security claim mapped to the test (or documented manual justification) that proves it. |
| [`openspec/`](openspec/) | The OpenSpec-driven development process: applied capability specs, in-flight changes, and the implementation sequence in [`openspec/openspine-change-sequence.md`](openspec/openspine-change-sequence.md). |
| [`openspec/conventions.md`](openspec/conventions.md) | Per-change ceremony: proposal → spec → design → tasks → archive. |

## Status

Alpha, and honest about it. The substrate and Lyra run end to end: gated actions, Telegram owner control, Gmail draft previews with digest-bound approval, and a governed artifact lifecycle for rules and routes. The [change sequence](openspec/openspine-change-sequence.md) records exactly what has landed and what hasn't; the [roadmap](https://george-rd.github.io/openspine/roadmap/) records what is deferred on purpose.

## License

Free to use. MIT or Apache 2.0 — pick whichever suits you.
