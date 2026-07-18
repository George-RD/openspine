---
title: Quickstart
description: Build OpenSpine, run every check locally, and stand up your own Lyra.
---

Everything runs on your machine. No account, no hosted service, no telemetry — clone it, build it, and the same checks that gate every merge run on your laptop.

## Build and prove it works

```sh
git clone https://github.com/George-RD/openspine.git
cd openspine
npm ci # dev tools used by the check script
cargo build --workspace
./scripts/check.sh # runs every test and check - same as CI
```

`check.sh` is the whole gate: formatting, lints, the full test suite, and the claims register that ties every documented security claim to a named test (or a recorded manual justification). If it passes for you, you're holding the same system we ship.

## Configure a real server

1. Copy `.env.example` to `.env` and fill in the values. This file holds your secrets; it is ignored by Git and must never be shared. At a minimum you need:
   - `OPENSPINE_TELEGRAM_BOT_TOKEN`: get one from [@BotFather](https://t.me/BotFather).
   - `OPENSPINE_ARTIFACT_KEY`: a random 32-byte key (`openssl rand -hex 32`). Every private message, email, and prompt is stored encrypted with it.
   - Your model provider API key (like `ANTHROPIC_API_KEY`).
2. Copy `openspine.docker.example.yaml` (for Docker) or `openspine.example.yaml` (for local running) to `openspine.yaml`. Set `owner.telegram_user_id` to your Telegram user ID — message [@userinfobot](https://t.me/userinfobot) to find it. This ID is what "owner" means to the runtime: only messages verified against it reach owner control.
3. Run `docker compose up --build` (or `cargo run -p openspine-kernel` for bare metal).

Full setup guides live in the repository:
- [docs/telegram-setup.md](https://github.com/George-RD/openspine/blob/main/docs/telegram-setup.md): the Telegram control channel.
- [docs/gmail-setup.md](https://github.com/George-RD/openspine/blob/main/docs/gmail-setup.md): the Gmail connection and the draft command.

## Talk to it

Send a direct message to your bot from your owner account:

- `/status` — is the server up and holding its invariants.
- `/draft <thread_id>` — fetch a Gmail thread (the thread ID from the Gmail web URL) and draft a reply. The draft is previewed on Telegram; tap Approve and the draft — exactly the text you saw, verified by digest — appears in Gmail. Sending remains yours.
- `/propose <kind>` followed by YAML — propose a new rule, route, or policy. It stays inert until you approve the exact text on screen.
