<picture>
  <img src="docs/readme-header.svg" width="100%" alt="OpenSpine. Let a personal AI do the job. Keep the rest of your accounts out of reach. A self-hosted personal AI system with hard task limits." />
</picture>

<p align="center">
  <a href="https://george-rd.github.io/openspine/"><strong>Website</strong></a>
  ·
  <a href="https://george-rd.github.io/openspine/comparison/"><strong>How it differs</strong></a>
  ·
  <a href="https://george-rd.github.io/openspine/quickstart/"><strong>Quickstart</strong></a>
  ·
  <a href="https://george-rd.github.io/openspine/threat-model/"><strong>Threat model</strong></a>
  ·
  <a href="https://george-rd.github.io/openspine/roadmap/"><strong>Roadmap</strong></a>
  ·
  <a href="https://ko-fi.com/george_builds"><strong>Support</strong></a>
</p>

# OpenSpine

**Let a personal AI do the job. Keep the rest of your accounts out of reach.**

OpenSpine comes with **Lyra, the assistant you talk to**. You choose the task. Lyra can use only what that task needs. Anything else stays out of reach.

If the promise of OpenClaw or Hermes appeals to you, but broad access to your inbox, files, and accounts does not, OpenSpine is built around that exact hesitation.

> **Alpha:** Choose one Gmail thread. Lyra drafts a reply. Only the exact draft you approve is created. Sending stays blocked.

OpenSpine is much narrower than OpenClaw or Hermes today. It does not run either project, and it does not yet support other assistants through a finished compatibility layer. Lyra is the path that works now.

## Why this needs a different system

A chat assistant can suggest what to do. A personal agent becomes useful when it can read mail, use files, update accounts, or run tools for you.

That is also the point where one mistake can reach something real.

The failure scenes are ordinary:

- a poisoned email tells the model to read more mail;
- the agent selects the wrong customer, thread, or account;
- a skill adds an extra recipient;
- a credential appears in tool output or chat history;
- an approval applies to something different from what you reviewed;
- the agent quietly gives itself more access.

Prompts, allowlists, approvals, and sandboxes all help. OpenSpine adds a harder rule: **the AI does not decide what it is allowed to reach.**

<picture>
  <img src="docs/readme-boundary.svg" width="100%" alt="Comparison of a common model-driven agent setup and OpenSpine. In the common setup, the agent process holds broad connector credentials and relies on prompt rules. With OpenSpine, the runtime holds credentials, gives the agent short-lived task permissions, and checks model-driven account actions before a connector runs." />
</picture>

## What happens when you ask Lyra to do something

```text
you ↔ Lyra
       │ asks for work
       ▼
OpenSpine runtime
  verifies → limits → grants → checks → records
       │
       ▼
email, models, and other connectors
```

- **Lyra** talks with you and coordinates the work.
- **The runtime** checks the request, decides what this task may use, holds the account keys, and checks actions.
- **Workers** receive a short-lived task pass and the context needed for that job. They do not receive raw connector keys.
- **AI-requested account actions** run only when the task allows them or the exact approval is satisfied.
- **A small set of owner-selected metadata reads before the main gate** is separately listed, classified, and audited.

The deterministic path is:

```text
event → verify → identify → route → compose → grant → run → gate → audit
```

A malicious email can change what the model tries to do. It cannot create permission to read another thread or send an email.

## The trade-off against OpenClaw and Hermes

OpenClaw and Hermes offer far more channels, tools, skills, automation, and onboarding than OpenSpine does today. They also provide real security controls.

OpenSpine makes a different trade:

> **Each task gets hard limits before the AI can reach your accounts.**

The distinction is structural, not a claim that the other projects ignore security. OpenSpine makes task permission a first-class runtime object and keeps it outside the model. AI-requested effects pass through one gate before dispatch. A small set of owner-selected pre-gate metadata reads is separately classified and audited.

See the [full comparison](https://george-rd.github.io/openspine/comparison/) for the current strengths and trade-offs of each approach.

## Lyra: the working proof

<picture>
  <img src="docs/readme-lyra-flow.svg" width="100%" alt="Lyra alpha flow. A verified Telegram request selects one Gmail thread. OpenSpine creates a task grant, Lyra drafts a reply, the owner approves the exact text, and Gmail receives a draft. Email sending follows a separate denied path." />
</picture>

Today, you send Lyra a Gmail thread ID in Telegram. OpenSpine verifies that the message came from you and binds the task to that thread. Lyra prepares a reply. You approve the exact text and target before Gmail receives a draft. `email.send` remains denied.

This workflow is deliberately narrow. It proves the boundary against hostile external content without claiming that the alpha is already a full chief-of-staff assistant.

## More access needs another decision

An agent can propose a new route, rule, workflow, or capability. The proposal stays inactive until the relevant lifecycle and approval checks pass. Nothing can silently give itself more access.

Skills currently install through a verified-owner command and a separate promotion lifecycle. Agent-proposed skill installation is not part of the public Lyra path today.

The broader design aims to make safe repetition smoother: do internal work freely, ask at a real effect boundary, and turn repeated approvals into revocable standing rules only after one explicit decision.

## Proof you can run

Each documented security claim points to a named test. `scripts/check-claims.sh` fails the build if a listed test disappears.

| Runtime claim | Named test |
|---|---|
| A spoofed owner ID without a verified source is denied | `spoofed_owner_id_without_verified_source_is_denied` |
| A task cannot read a different Gmail thread | `email_read_selected_thread_rejects_foreign_grant` |
| The agent process receives no raw connector credentials | `process_driver_clears_env_and_sets_only_two_vars` |
| An explicit deny overrides an allow | `explicit_deny_overrides_allow` |
| An approval-required worker action stops before dispatch | `approval_required_action_stops_before_dispatch` |
| Email sending is denied in every grant and approval state | `global_policy_round_trips_and_denies_send` |

The [full claims register](docs/threat-claims.md) covers external content, model calls, approval binding, audit artifacts, host operations, and more.

## Current trade-offs

Choose the alpha because the task boundary matters enough to accept:

- Docker, Telegram, model-provider, and Gmail OAuth setup;
- a copied Gmail thread ID instead of a polished picker;
- one narrow owner-facing workflow;
- fewer channels and tools than mature agent platforms;
- an architecture and threat model that are still evolving in public.

Do not choose it today if your main requirement is the widest assistant feature set or a consumer-grade setup flow.

## Build and run the checks

```sh
git clone https://github.com/George-RD/openspine.git
cd openspine
npm ci
cargo build --workspace
./scripts/check.sh
```

This runs formatting, lints, tests, strict OpenSpec validation, and the claims register used by CI. The [quickstart](https://george-rd.github.io/openspine/quickstart/) then covers Telegram, Gmail, and model setup.

## Run Lyra

For Telegram control and model replies, the server needs:

- `OPENSPINE_TELEGRAM_BOT_TOKEN`
- `OPENSPINE_ARTIFACT_KEY`, generated with `openssl rand -hex 32`
- credentials for Anthropic, OpenAI, or another compatible model provider

For Gmail drafting, follow the [Gmail setup guide](docs/gmail-setup.md). It adds the Google OAuth client, `OPENSPINE_GMAIL_CLIENT_SECRET`, `OPENSPINE_GMAIL_REFRESH_TOKEN`, and a `gmail:` block with your `mailbox_address`.

Copy `.env.example` to `.env` and put the secret values there. Set `DOCKER_GID` to the numeric group ID of `/var/run/docker.sock` (`stat -c '%g' /var/run/docker.sock` on Linux or `stat -f '%g' /var/run/docker.sock` on macOS). Then copy `openspine.docker.example.yaml` to `openspine.yaml` and set `owner.telegram_user_id` plus the Gmail fields.

Build the contained task worker, then start the kernel:

```sh
docker build --file Dockerfile.shell --tag openspine-shell:latest .
docker compose up --build
```

Compose mounts the Lyra package read-only and retains runtime state in `./data`. A one-shot initializer fixes that directory's ownership before the non-root kernel starts, so existing Compose data is preserved across upgrades. The bare-metal process driver is a development shortcut; `/draft` is refused unless `unsafe_allow_uncontained_private_data: true` is set in an isolated development config.

Then message your bot:

```text
/draft <gmail_thread_id>
```

## Documentation

| Document | What it covers |
|---|---|
| [Why OpenSpine](https://george-rd.github.io/openspine/why-openspine/) | The real-account trust gap and the boundary OpenSpine enforces. |
| [How OpenSpine differs](https://george-rd.github.io/openspine/comparison/) | A fair comparison with capability-first personal agents. |
| [Architecture](https://george-rd.github.io/openspine/architecture/) | The event path, task grants, gate, artifacts, and audit model. |
| [`docs/threat-claims.md`](docs/threat-claims.md) | Every security claim and the test or manual proof behind it. |
| [`.raw/openspine-layperson-offer-rerun-2026-07-30.md`](.raw/openspine-layperson-offer-rerun-2026-07-30.md) | The full Growth Arsenal rerun, buyer personas, value equation, adversarial findings, and paired copy evaluation. |
| [`.raw/openspine-positioning-audit-2026-07-30.md`](.raw/openspine-positioning-audit-2026-07-30.md) | The earlier architecture and positioning audit that this reader-first rerun corrects. |
| [`.raw/openspine-decision-log.md`](.raw/openspine-decision-log.md) | Architecture decisions, consequences, and reversal conditions. |
| [`openspec/openspine-change-sequence.md`](openspec/openspine-change-sequence.md) | What has landed, what comes next, and the order of work. |

## Status

Alpha. The OpenSpine system and Lyra run end to end for verified owner control, scoped Gmail reads, reply previews, digest-bound draft approval, gated actions, and governed changes. The [change sequence](openspec/openspine-change-sequence.md) records runtime work that has landed. The [roadmap](https://george-rd.github.io/openspine/roadmap/) separates that from the owner-facing product work still missing.

## License

Free to use. MIT or Apache 2.0, whichever suits you.
