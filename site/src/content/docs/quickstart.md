---
title: Quickstart
description: Build OpenSpine and run a local Lyra instance.
---

## Build and run the check script

```sh
git clone https://github.com/George-RD/openspine.git
cd openspine
npm ci # dev tools used by the check script
cargo build --workspace
./scripts/check.sh # runs every test and check - same as CI
```

The check script runs all local tests and checks. This is the exact same check that runs on GitHub.

## Configure a real server

1. Copy `.env.example` to `.env` and fill in the values. This file holds your secrets. It is ignored by Git and must never be shared. At a minimum, you need:
   - `OPENSPINE_TELEGRAM_BOT_TOKEN`: Get this from [@BotFather](https://t.me/BotFather).
   - `OPENSPINE_ARTIFACT_KEY`: A random 32-byte key. You can make one by running `openssl rand -hex 32`. Every private message, email, and prompt is stored encrypted with this key.
   - Your model provider API key (like `ANTHROPIC_API_KEY`).
2. Copy `openspine.docker.example.yaml` (for Docker) or `openspine.example.yaml` (for local running) to `openspine.yaml`. Set `owner.telegram_user_id` to your Telegram user ID. You can find your ID by messaging [@userinfobot](https://t.me/userinfobot).
3. Run `docker compose up --build`. (For a bare-metal run, run `cargo run -p openspine-kernel`).

Full setup guides live in the repository:
- [docs/telegram-setup.md](https://github.com/George-RD/openspine/blob/main/docs/telegram-setup.md): Setting up the Telegram control channel.
- [docs/gmail-setup.md](https://github.com/George-RD/openspine/blob/main/docs/gmail-setup.md): Setting up Gmail connection and the draft command.

## Try it

Send a direct message to your bot from your Telegram owner account:

- `/status`: Checks server status.
- `/draft <thread_id>`: Fetches a Gmail thread (using the thread ID from the Gmail web URL) and drafts a reply. The reply is previewed on Telegram. Tap Approve to create the draft in Gmail.
- `/propose <kind>` followed by YAML rules: Proposes a new rule, route, or policy. The rule stays inactive until you approve the exact text you see.
