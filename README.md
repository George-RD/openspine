<picture>
  <img src="docs/readme-header.svg" width="100%" alt="OpenSpine. Let AI use your tools. Keep the keys. A self-hosted permission layer gives each task limited access and checks actions before email or another tool runs." />
</picture>

<p align="center">
  <a href="https://george-rd.github.io/openspine/"><strong>Website</strong></a>
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

**A self-hosted permission layer for AI assistants.**

OpenSpine lets an AI assistant use your email and other tools. The model does not get your account keys. Each task gets a small set of temporary permissions. OpenSpine checks each action, asks you when needed, and records the result.

> **Alpha:** Lyra can read one Gmail thread you choose and create a draft after you approve the exact text. Email sending is blocked by runtime policy.

<picture>
  <img src="docs/readme-boundary.svg" width="100%" alt="Comparison of a common model-driven agent setup and OpenSpine. In the common setup, the agent process holds broad connector credentials and relies on prompt rules. With OpenSpine, the runtime holds credentials, gives the agent a short-lived task grant, and allows, asks about, or denies each action before a connector runs." />
</picture>

## What OpenSpine changes

A common setup puts broad connector access in the same process the model can steer and relies on prompt rules. OpenSpine keeps credentials in the runtime. The agent receives a short-lived task grant. The model can only request actions inside those limits.

- The source is verified before owner identity is trusted.
- Routes, agent rules, workflows, capabilities, and policy combine into the task grant.
- One gate returns allow, ask, or deny before any connector runs.
- Decisions point to encrypted artifacts in a hash-chained audit log.
- An agent can propose more access, but only you can activate it.

The exact runtime path is deterministic:

```text
event → verify → identify → route → compose → grant → run → gate → audit
```

A malicious email can change what the model tries to do. It cannot change what OpenSpine allows.

## Lyra: the working example

<picture>
  <img src="docs/readme-lyra-flow.svg" width="100%" alt="Lyra alpha flow. A verified Telegram request selects one Gmail thread. OpenSpine creates a task grant, Lyra drafts a reply, the owner approves the exact text, and Gmail receives a draft. Email sending follows a separate denied path." />
</picture>

Today, you send Lyra a Gmail thread ID in Telegram. OpenSpine verifies the owner message, scopes the task to that thread, and lets Lyra prepare a reply. You approve the exact text before a Gmail draft is created. `email.send` remains denied.

## Permissions grow only after you approve them

An agent can propose a new route, rule, or capability. The proposal stays inactive until you approve the exact, digest-bound content. Nothing can silently give itself more access.

## Proof you can run

Each documented safety claim points to a named test. `scripts/check-claims.sh` fails the build if a listed test disappears.

| Runtime claim | Named test |
|---|---|
| A spoofed owner ID without a verified source is denied | `spoofed_owner_id_without_verified_source_is_denied` |
| A task cannot read a different Gmail thread | `email_read_selected_thread_rejects_foreign_grant` |
| The agent process receives no raw connector credentials | `process_driver_clears_env_and_sets_only_two_vars` |
| An explicit deny overrides an allow | `explicit_deny_overrides_allow` |
| Every effectful action stops at the gate before dispatch | `approval_required_action_stops_before_dispatch` |
| Email sending is denied in every grant and approval state | `global_policy_round_trips_and_denies_send` |

The [full claims register](docs/threat-claims.md) covers external content, model calls, approval binding, audit artifacts, host operations, and more.

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

Compose mounts the Lyra package into the kernel read-only and stores runtime data in a Docker-managed volume. The bare-metal process driver is a development shortcut; `/draft` is refused unless `unsafe_allow_uncontained_private_data: true` is set in an isolated development config.

Then message your bot:

```text
/draft <gmail_thread_id>
```

## Documentation

| Document | What it covers |
|---|---|
| [Why OpenSpine](https://george-rd.github.io/openspine/why-openspine/) | The problem with prompt-only safety and the runtime boundary OpenSpine enforces. |
| [Architecture](https://george-rd.github.io/openspine/architecture/) | The event path, task grants, gate, artifacts, and audit model. |
| [`docs/threat-claims.md`](docs/threat-claims.md) | Every security claim and the test or manual proof behind it. |
| [`.raw/openspine-decision-log.md`](.raw/openspine-decision-log.md) | Architecture decisions, consequences, and reversal conditions. |
| [`openspec/openspine-change-sequence.md`](openspec/openspine-change-sequence.md) | What has landed, what comes next, and the order of work. |

## Status

Alpha. The permission layer and Lyra run end to end: verified owner control, scoped Gmail reads, reply previews, digest-bound draft approval, gated actions, and governed changes to rules and routes. The [change sequence](openspec/openspine-change-sequence.md) records what has landed. The [roadmap](https://george-rd.github.io/openspine/roadmap/) records what is still missing or deferred.

## License

Free to use. MIT or Apache 2.0, whichever suits you.
