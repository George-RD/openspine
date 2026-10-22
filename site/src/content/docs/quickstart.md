---
title: Quickstart
description: Build the OpenSpine system, run its proof checks, and start the included Lyra assistant.
---

This quickstart installs the OpenSpine system from source. Lyra is the included assistant package. The governed runtime underneath Lyra keeps credentials and task authority outside the model.

Everything runs on your machine. There is no OpenSpine account, hosted service, or telemetry. The current alpha setup is technical and the first useful workflow is narrow.

## Build and prove it works

```sh
git clone https://github.com/George-RD/openspine.git
cd openspine
npm ci # dev tools used by the check script
cargo build --workspace
./scripts/check.sh # runs every test and check used by CI
```

`check.sh` runs formatting, lints, the full test suite, strict OpenSpec validation, and the claims register that ties every documented security claim to a named test or recorded manual justification.

## First run (local chat)

One command turns the built binary into a running, governed install. `openspine init` writes a configuration and an owner-only key file, binds you as the single trusted owner, and prints the trust ceremony — seed key, approval boundary, and how to test it:

```sh
cargo build --workspace --bins
export PATH="$PWD/target/debug:$PATH"
openspine --config openspine.local.yaml init --owner <telegram_user_id> --name "Your Name" --model <model_id>
openspine --config openspine.local.yaml chat
```

`--owner` is your Telegram user id — the trusted principal every later approval and audit row is bound to. Message [@userinfobot](https://t.me/userinfobot) to find it. `--model` is the model your OpenAI-compatible endpoint serves (default `http://127.0.0.1:11434`, override with `--base-url`); without it `init` asks the endpoint and uses a lone answer, otherwise it stops and asks you to choose. The configuration it writes uses absolute paths, so it works from any directory. This is the fastest path to a governed local reply; the [first-run guide](https://github.com/George-RD/openspine/blob/main/docs/first-run.md) explains each step of the ceremony and the capability rungs beyond it. The Docker deployment below adds Telegram and the guarded Gmail draft flow.

## Configure a real server

1. Copy `.env.example` to `.env` and fill in the values. Compose reads this one file and passes it into the kernel container; it does not read the `openspine.env` that `init` writes for the bare-metal path. At minimum you need:
   - `DOCKER_GID`: the group ID of the Docker socket as the Linux daemon sees it. Don't `stat` the host path: on Docker Desktop and OrbStack that is a symlink into a VM and reports the wrong group. Ask a container instead:
     ```sh
     docker run --rm -v /var/run/docker.sock:/var/run/docker.sock busybox:1.36 \
       stat -c '%g' /var/run/docker.sock
     ```
     This prints `0` on OrbStack and the `docker` group ID on Linux.
   - `OPENSPINE_TELEGRAM_BOT_TOKEN`: get one from [@BotFather](https://t.me/BotFather).
   - `OPENSPINE_ARTIFACT_KEY`, `OPENSPINE_GRANT_HMAC_KEY` and `OPENSPINE_WEBHOOK_HMAC_KEY`: three separate random 32-byte keys, each from `openssl rand -hex 32`. If you already ran `init` on this data directory, copy these three lines from `openspine.env` instead.
   - Your model provider credentials, such as `ANTHROPIC_API_KEY`.
   - Optional: `RUST_LOG`. The kernel logs at `info` by default; set `RUST_LOG=debug` (or a narrower filter) for more detail.
2. Copy `openspine.docker.example.yaml` to `openspine.yaml`. Set `owner.telegram_user_id` to your Telegram user ID. Message [@userinfobot](https://t.me/userinfobot) to find it.
3. To use `/draft`, follow the [Gmail setup guide](https://github.com/George-RD/openspine/blob/main/docs/gmail-setup.md):
   - fill `OPENSPINE_GMAIL_CLIENT_SECRET` and `OPENSPINE_GMAIL_REFRESH_TOKEN` in `.env`;
   - add the `gmail:` block to `openspine.yaml`, including your `mailbox_address`.
4. Build the contained task-worker image expected by the Docker configuration, then start the kernel:

```sh
docker build --file Dockerfile.shell --tag openspine-shell:latest .
docker compose up --build
```

Compose mounts the Lyra package read-only and retains runtime state in `./data`. The kernel's lifetime lock and terminal-erasure ledger live in `./control` (`OPENSPINE_CONTROL_DIR`, set by `compose.yaml`), so recreating the container keeps them with `./data`; back up and restore the two directories together. A deployment that ran before this directory existed has its ledger only inside the old container: migrate it before the first `docker compose up` with the new `compose.yaml`, or the kernel refuses to start with `terminal-erasure continuity is missing`. Stop the old kernel first so the ledger cannot change mid-copy: `docker compose stop kernel`, `mkdir -p control`, find the name with `docker cp "$(docker compose ps -aq kernel)":/app - | tar -t | grep -m1 openspine-control` (lists names only; nothing from `./data` is written to disk), copy it with `docker cp "$(docker compose ps -aq kernel)":/app/.openspine-control-<hash> ./control/` (`docker cp` works on a stopped container), then `docker compose rm -f kernel` so the old copy can never be restarted, and finally `docker compose up -d`. A one-shot initializer fixes that directory's ownership before the non-root kernel starts, so existing Compose data is preserved across upgrades. Docker is the supported path for the Gmail workflow. The bare-metal process driver is a development shortcut and refuses `/draft` unless `unsafe_allow_uncontained_private_data: true` is set in an isolated development config.

Full setup guides:

- [Telegram setup](https://github.com/George-RD/openspine/blob/main/docs/telegram-setup.md)
- [Gmail setup](https://github.com/George-RD/openspine/blob/main/docs/gmail-setup.md)

## Talk to Lyra

Send a direct message to your bot from the configured owner account:

- `/status` checks whether the system is up and holding its invariants.
- `/draft <thread_id>` reads only the selected Gmail thread and prepares a reply. Telegram shows the exact text for approval before Gmail receives a draft. Email sending remains denied.
- `/propose <kind>` followed by YAML proposes a new rule, route, or policy. It stays inactive until you approve the exact text.

`openspine init` collapses local first-run into one command ([issue #118](https://github.com/George-RD/openspine/issues/118)); the remaining Docker/Telegram/Gmail setup and the intended package-level install and run flow are tracked in [issue #117](https://github.com/George-RD/openspine/issues/117).