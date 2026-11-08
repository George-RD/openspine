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

One command turns the built binary into a running, governed install. `openspine init` writes a configuration and an owner-only key file, binds you as the single trusted owner, and explicitly accepts the four byte-exact embedded seed workflows for that owner through the ordinary auditable artifact lifecycle. It prints the trust ceremony — seed key, approval boundary, and how to test it:

```sh
cargo build --workspace --bins
export PATH="$PWD/target/debug:$PATH"
openspine --config openspine.local.yaml init --owner <telegram_user_id> --name "Your Name" --model <model_id>
openspine --config openspine.local.yaml chat
```

`--owner` is your Telegram user id — the trusted principal every later approval and audit row is bound to. Message [@userinfobot](https://t.me/userinfobot) to find it. `--model` is the model your OpenAI-compatible endpoint serves (default `http://127.0.0.1:11434`, override with `--base-url`); without it `init` asks the endpoint and uses a lone answer, otherwise it stops and asks you to choose. The configuration it writes uses absolute paths, so it works from any directory. This is the fastest path to a governed local reply; the [first-run guide](https://github.com/George-RD/openspine/blob/main/docs/first-run.md) explains each step of the ceremony and the capability rungs beyond it. The Docker deployment below adds Telegram and the guarded Gmail draft flow.

The interactive `openspine setup` first-run wizard asks separately before accepting seeds (default no); setup on an existing configuration and `setup --check` do not accept them. Without consent the seeds stay in quarantine. Re-running `init` accepts only unchanged embedded seeds parked for legacy reconfirmation and consumes those matching requests transactionally; edited content and unrelated reviews remain for owner review, and deleted or revoked seeds are not restored. The customer-service template is accepted with its dangling dependencies recorded but stays inert until its agent and pack exist. The [first-run guide](https://github.com/George-RD/openspine/blob/main/docs/first-run.md#explicit-seed-workflow-acceptance-d-175) explains the exact digest/id/version/owner binding.

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
4. Build the contained task-worker image, run the explicit seed-acceptance ceremony for the configured owner, then start the kernel. For an existing deployment, stop the kernel first and complete the control-ledger migration below before running init:

```sh
docker build --file Dockerfile.shell --tag openspine-shell:latest .
docker compose run --rm --build kernel --config /app/openspine.yaml init
docker compose up --build
```

This init command uses the model already set in the Docker example and all three keys from `.env`. Omit `--model`/`--base-url`: the existing `/app/openspine.yaml` is mounted read-only and does not need rewriting. Acceptance is persisted in `./data`; `data-init` prepares ownership of the mounted data/control directories. `docker compose up` alone does not accept seeds and retains the quarantine/reconfirmation path. These commands follow the checked-in mounts and CLI, not an exercised Docker run.

Compose mounts the Lyra package read-only and retains runtime state in `./data`. The kernel's lifetime lock and terminal-erasure ledger live in `./control` (`OPENSPINE_CONTROL_DIR`, set by `compose.yaml`), so recreating the container keeps them with `./data`; back up and restore the two directories together. A deployment that ran before this directory existed has its ledger only inside the old container: migrate it before the first `docker compose up` with the new `compose.yaml`, or the kernel refuses to start with `terminal-erasure continuity is missing`. Stop the old kernel first so the ledger cannot change mid-copy: `docker compose stop kernel`, `mkdir -p control`, find the name with `docker cp "$(docker compose ps -aq kernel)":/app - | tar -t | grep -m1 openspine-control` (lists names only; nothing from `./data` is written to disk), copy it with `docker cp "$(docker compose ps -aq kernel)":/app/.openspine-control-<hash> ./control/` (`docker cp` works on a stopped container), then `docker compose rm -f kernel` so the old copy can never be restarted, and finally `docker compose up -d`. A one-shot initializer fixes that directory's ownership before the non-root kernel starts, so existing Compose data is preserved across upgrades. Docker is the supported path for the Gmail workflow. The bare-metal process driver is a development shortcut and refuses `/draft` unless `unsafe_allow_uncontained_private_data: true` is set in an isolated development config.

Full setup guides:

- [Telegram setup](https://github.com/George-RD/openspine/blob/main/docs/telegram-setup.md)
- [Gmail setup](https://github.com/George-RD/openspine/blob/main/docs/gmail-setup.md)

## Talk to Lyra

Send a direct message to your bot from the configured owner account:

- `/status` checks whether the system is up and holding its invariants.
- `/draft <thread_id>` reads only the selected Gmail thread and prepares a reply. Telegram shows the exact text for approval before Gmail receives a draft. Email sending remains denied.
- `/propose <kind>` followed by YAML proposes a new rule, route, or policy. It stays inactive until you approve the exact text.

## Select the package used at next startup

For an initialized instance, stop the service before package maintenance:

```sh
openspine install lyra --json
openspine setup --check
openspine package review "$INSTALLATION_ID" --json
openspine use "$INSTALLATION_ID"
openspine setup --check
openspine chat --once "Hello again"
```

Use the exact installation ID in the receipt and type the displayed
`select <installation-id>` phrase. Installation does not select or grant
permissions. A completed chat grant normally lasts 120 seconds; wait for it and
resolve other durable work normally before selecting. Never clear an unknown
external-effect fence to force a switch.

Readiness reports the legacy configured base, installed-inactive candidates or
the committed selection with exact identity and local provenance (not publisher
authentication). The package check copies a short read-only snapshot, closes it
before object I/O and takes no lifetime lock. The command's vault opener takes the
lock when free or degrades to `Locked` alongside the service. Database contention
reports `database-busy`: retry. Missing, corrupt or incompatible selections fail
closed; managed mode ignores `lyra_dir` even if the old source disappears.

Selection trusts the account controlling owner-only local state, accepts only
the same product and applies at next normal startup—no hot reload or separate
`run` engine. An epoch change requires ordinary learned-artifact reconfirmation
and standing-rule review. Reviewed recovery is `openspine use "$EARLIER_INSTALLATION_ID"`,
never an older database restore or manual `lyra_dir` edit; later revocations,
pauses, erasures, approvals and budget history remain.

The local installation/selection/readiness path of
[epic #117](https://github.com/George-RD/openspine/issues/117) is shipped.
Signed/version-pinned distribution, remote fetch, cross-product migration and
Bell integration remain unfinished. See the
[operator guide](https://github.com/George-RD/openspine/blob/main/docs/package-installation.md#operator-flow-and-readiness)
for bootstrap prerequisites, service locks, grant TTLs and recovery boundaries.