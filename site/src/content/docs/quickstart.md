---
title: Quickstart
description: Build OpenSpine, run every check locally, and stand up your own Lyra.
---

Everything runs on your machine. No account, no hosted service, no telemetry. Clone it, build it, and run the same checks that gate every merge.

## Build and prove it works

```sh
git clone https://github.com/George-RD/openspine.git
cd openspine
npm ci # dev tools used by the check script
cargo build --workspace
./scripts/check.sh # runs every test and check used by CI
```

`check.sh` runs formatting, lints, the full test suite, strict OpenSpec validation, and the claims register that ties every documented safety claim to a named test or recorded manual justification.

## Configure a real server

1. Copy `.env.example` to `.env` and fill in the values. Compose passes this file into the kernel container. At minimum you need:
   - `DOCKER_GID`: the numeric group ID of `/var/run/docker.sock`. Use `stat -c '%g' /var/run/docker.sock` on Linux or `stat -f '%g' /var/run/docker.sock` on macOS.
   - `OPENSPINE_TELEGRAM_BOT_TOKEN`: get one from [@BotFather](https://t.me/BotFather).
   - `OPENSPINE_ARTIFACT_KEY`: a random 32-byte key from `openssl rand -hex 32`.
   - Your model provider credentials, such as `ANTHROPIC_API_KEY`.
2. Copy `openspine.docker.example.yaml` to `openspine.yaml`. Set `owner.telegram_user_id` to your Telegram user ID. Message [@userinfobot](https://t.me/userinfobot) to find it.
3. To use `/draft`, follow the [Gmail setup guide](https://github.com/George-RD/openspine/blob/main/docs/gmail-setup.md):
   - fill `OPENSPINE_GMAIL_CLIENT_SECRET` and `OPENSPINE_GMAIL_REFRESH_TOKEN` in `.env`;
   - add the `gmail:` block to `openspine.yaml`, including your `mailbox_address`.
4. Build the contained task-worker image expected by the Docker configuration, then start the kernel:

```sh
docker build --file Dockerfile.shell --tag openspine-shell:latest .
docker compose up --build
```

Compose mounts the Lyra package into the kernel read-only and stores runtime data in a Docker-managed volume. Docker is the supported path for the Gmail workflow. The bare-metal process driver is a development shortcut and refuses `/draft` unless `unsafe_allow_uncontained_private_data: true` is set in an isolated development config.

Full setup guides:

- [Telegram setup](https://github.com/George-RD/openspine/blob/main/docs/telegram-setup.md)
- [Gmail setup](https://github.com/George-RD/openspine/blob/main/docs/gmail-setup.md)

## Talk to it

Send a direct message to your bot from the configured owner account:

- `/status` checks whether the server is up and holding its invariants.
- `/draft <thread_id>` reads only the selected Gmail thread and prepares a reply. Telegram shows the exact text for approval before Gmail receives a draft. Email sending remains denied.
- `/propose <kind>` followed by YAML proposes a new rule, route, or policy. It stays inactive until you approve the exact text.
