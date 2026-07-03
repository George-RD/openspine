---
title: Quickstart
description: Build OpenSpine and stand up a local Lyra instance.
---

## Build and run the local gate

```sh
git clone https://github.com/George-RD/openspine.git
cd openspine
cargo build --workspace
./scripts/check.sh    # fmt, clippy -D warnings, tests, file-size gate, claims gate, openspec validate --all --strict
```

`scripts/check.sh` is the real gate — it runs everything CI would, plus the
[threat-claims register](/threat-model/) check, locally.

## Configure a real kernel instance

1. Copy `.env.example` to `.env` and fill in real values — this file is
   gitignored and must never be committed. At minimum you need:
   - `OPENSPINE_TELEGRAM_BOT_TOKEN` — from
     [`@BotFather`](https://t.me/BotFather).
   - `OPENSPINE_ARTIFACT_KEY` — 32 random bytes, hex-encoded
     (`openssl rand -hex 32`). Every private payload (a raw Telegram
     message, an email body, a model prompt/output, a draft body) is
     stored encrypted at rest with this key.
   - A model-provider API key (e.g. `ANTHROPIC_API_KEY`).
2. Copy `openspine.docker.example.yaml` (Docker deployment) or
   `openspine.example.yaml` (local/dev) to `openspine.yaml` and set
   `owner.telegram_user_id` to your own numeric Telegram user ID (message
   [`@userinfobot`](https://t.me/userinfobot) to find it — identity is not
   authority, but the kernel still needs to know who the owner is).
3. `docker compose up --build` (or `cargo run -p openspine-kernel` for a
   bare-metal dev run under the unsafe `process` sandbox driver).

Full walkthroughs, including the Gmail connector setup and the exact
`openspine.yaml` shape, live in the repository:

- [`docs/telegram-setup.md`](https://github.com/George-RD/openspine/blob/main/docs/telegram-setup.md) — the owner-control channel, step by step.
- [`docs/gmail-setup.md`](https://github.com/George-RD/openspine/blob/main/docs/gmail-setup.md) — the selected-thread Gmail connector and the `/draft <thread_id>` command.

## Try it

DM your bot from the owner Telegram account:

- `/status` — reads kernel status through the gate-mediated action API.
- `/draft <thread_id>` — fetches a Gmail thread (its id is the trailing hex
  string in Gmail's own web UI URL) and drafts a reply, previewed over
  Telegram. Tap "Approve" to create the exact reviewed Gmail draft — no
  email is ever sent by this flow.
- `/propose <kind>` followed by a YAML artifact on the next line — proposes
  a new route, agent, workflow, capability pack, or policy. It stays inert
  until you approve the exact YAML via the same digest-bound button.
