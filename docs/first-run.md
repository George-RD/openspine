# First run: the trust ceremony

`openspine init` is the single command that turns a built binary into a
running, governed install. It is a trust loop, not a repository-setup exercise:
one command establishes one trusted principal and reports the boundary before
you send Lyra any work.

```bash
openspine init --owner <telegram_user_id> --name "Your Name" --model <model_id>
```

`--owner` is your Telegram user id — the single trusted principal every later
approval and audit row is bound to. Message
[@userinfobot](https://t.me/userinfobot) to find yours. `--name` is your
display name and defaults to `$USER`. Pass `--config <path>` to place the
configuration somewhere other than `openspine.yaml` in the current directory.

`--model` is the model id the default provider serves, and `--base-url` the
OpenAI-compatible endpoint (default `http://127.0.0.1:11434`, a local Ollama).
Without `--model`, a fresh `init` asks the endpoint what it serves and uses the
model it lists if there is exactly one. Several models, or an endpoint that
cannot be reached, stops `init` with a message naming `--model` and writes
nothing: a configuration with no model answers nothing. Re-running `init
--model <id>` on an existing configuration changes its model. Invoking `init`
also explicitly accepts the four shipped seed workflows as described below.

Everything `init` writes is location-independent: `data_dir` and the initial
legacy `lyra_dir` are absolute paths. If it cannot find the bundled Lyra source,
readiness names the legacy path to set. After an owner-reviewed package selection,
readiness and startup instead validate the committed retained object; removing or
changing the obsolete legacy source cannot override it.

The command is non-interactive and idempotent: re-running it fills in anything
missing without overwriting key material or the bound owner identity. Seed
materialization is one-time; it does not restore seeds you later delete.

## What the ceremony establishes

The three steps map one-to-one onto the runtime's trust model.

### 1. Seed key

`init` writes an owner-only key file, `openspine.env`, beside the
configuration at mode `0600`, containing three freshly generated 32-byte keys:

- `OPENSPINE_ARTIFACT_KEY` encrypts the credential vault, the artifact store,
  and the counterparty key ring.
- `OPENSPINE_GRANT_HMAC_KEY` signs the short-lived task grants the kernel hands
  to workers.
- `OPENSPINE_WEBHOOK_HMAC_KEY` signs webhook callbacks.

These keys stay in the kernel. They are never exported into a worker
environment. Every `openspine` command loads `openspine.env` before reading key
material, and a value already present in the environment always wins over the
file, so an operator-supplied key is never clobbered.

> The artifact key is the root of the vault. If a data directory already holds
> encrypted state, `init` refuses to mint a new artifact key rather than
> silently making that vault unreadable. Keep the original `openspine.env`.

### 2. Approval anchor

`init` binds the single trusted owner principal into a fresh kernel store. This
is the identity that ratifies work: Lyra runs each task under a short-lived
grant, and an external effect — for example sending mail — stops for your
approval and stays denied until you approve the exact action.

The owner identity is bound once and is **immutable**. The kernel fails closed
if the configured owner id later changes, so `init` requires the real id up
front on a fresh install and rejects an `--owner` that conflicts with an
already-bound identity. To change owners, start from a fresh data directory.

On both fresh and existing installs, `init` acquires the data-root lifetime lock
and validates the existing stores and owner before recording acceptance. Stop a
running kernel before re-running it; init is not a concurrent repair command.

#### Explicit seed-workflow acceptance (D-175)

Running `init` is your explicit consent to accept these four embedded workflows
for the bound owner:

- `owner_control_conversation_seed`
- `email_draft_with_approval_seed`
- `research_and_brief_seed`
- `customer_service_intake_seed`

Acceptance binds each exact embedded byte digest, artifact id and version, and
your owner principal. It records ordinary learned-artifact `OwnerAccepted`
provenance, an Active proposal, the dependency fingerprint, and audit evidence.
The three runnable seeds then pass through normal overlay admission; no runtime
auto-trust path is added and effects still require their normal authority.

The customer-service seed is accepted with its current dangling agent and pack
references recorded in that fingerprint. It stays inert until those dependencies
exist; acceptance does not install them.

The interactive `openspine setup` first-run wizard asks separately whether to
accept these seeds, defaulting to no. Declining leaves the ordinary quarantine/
owner-reconfirmation path. Running setup on an existing configuration, or
`setup --check`, does not accept seeds; use `init` to give that explicit consent.
Starting the kernel without ever running an accepting ceremony also leaves
seeds in quarantine.

On an existing install, re-running `init` converges only parked
`LegacyMigration` / `ReconfirmationRequired` seeds whose identity, version,
recorded digest, and current bytes still match the embedded seed. Acceptance and
consumption of their pending `artifact.reconfirm` requests happen in the same
transaction. Modified seeds and unrelated reviews stay in the normal owner-review
path. Already materialized seeds you deleted are not recreated, and revoked or
superseded proposals are not reactivated. Later edits or dependency drift face
the same checks as any other learned artifact.

#### Compose with the read-only configuration

After filling `.env` and copying/editing `openspine.docker.example.yaml` as
`openspine.yaml`, run the accepting ceremony before the first server start:

```sh
docker compose run --rm --build kernel --config /app/openspine.yaml init
docker compose up --build
```

The existing Docker example sets a model and absolute `/app/data` and
`/app/artifacts/lyra` paths. With those values already configured, omit `--model`
and `--base-url`: init need not rewrite the read-only `/app/openspine.yaml`.
Compose supplies all three keys from `.env`, so init need not create the adjacent
`openspine.env` either. It writes acceptance to the mounted data directory; the
`data-init` dependency prepares data/control ownership. Build the shell image as
shown in the [README](../README.md#run-the-gmail-drafting-proof) before serving.
For an existing deployment, stop the kernel and follow the
[control-ledger migration guidance](day-2-operations.md#seed-acceptance-and-existing-installs-d-175)
before init. These commands reflect the checked-in mounts and CLI; they are not
a report of an exercised Docker run.

### 3. Test

`init` prints a readiness report and the plain-language trust ceremony. A green
report means the configuration, seed keys, a configured model, and the Lyra
package are in place. To
close the loop with a governed reply:

```bash
openspine chat
```

Inside chat, `/status` reprints readiness and any message becomes a verified
owner request that runs through the full grant, gate, and audit path. See
[Direct terminal chat](terminal-chat.md) for the model-provider configuration
the first reply needs.

## After the first loop

### Select or recover an installed revision

Installation is not selection. Stop the service, install a candidate, inspect its
receipt and review/use its exact installation ID:

```sh
openspine install lyra --json
openspine setup --check
openspine package review "$INSTALLATION_ID" --json
openspine use "$INSTALLATION_ID"
openspine setup --check
```

The package check in `setup --check` uses a short read-only snapshot; its separate
vault opener takes the lifetime lock when free or degrades to `Locked` alongside
the service. Install/review/use require the exclusive lock. A completed first chat
normally leaves a 120-second Lyra task grant: wait for its expiry before `use`.
Other durable work, including unresolved external effects, also blocks switching.
Do not clear fences or delete work to bypass a blocker.

The first `use` explicitly adopts the configured declared base and supports only
the same product. It assumes the local account controls owner-only state and
keys, asks for the exact `select <installation-id>` phrase, and changes the next
startup, not the running process. Compatibility-epoch changes require ordinary
learned-artifact reconfirmation and standing-rule review.

Recovery uses `openspine use "$EARLIER_INSTALLATION_ID"` and a normal restart;
never restore an older database or edit `lyra_dir`. The existing owner,
credentials, history, receipts and later revocations/pauses/erasures/consumption/
budget controls remain. See [operator flow and readiness](package-installation.md#operator-flow-and-readiness)
and [selection restrictions](package-selection.md#rollout-restrictions).

### Connect optional capabilities

`init` reaches the local trust anchor without any network access. The
capability rungs beyond it need their own credentials and are reported as next
steps, not failures:

- **Model provider** — authorize a model with `openspine provider login`
  (`anthropic` or `openai-codex`), or point the `local` provider at an
  OpenAI-compatible endpoint such as Ollama. Required before Lyra can reply.
- **Telegram** — reach Lyra from your phone. See
  [Telegram setup](telegram-setup.md). Your `--owner` id is already bound.
- **Gmail draft** — the first connector-backed workflow, over the Docker
  deployment. See [Gmail setup](gmail-setup.md) and the
  [day-2 operations guide](day-2-operations.md).

The setup flow is deliberately truthful about what is required now, what is an
optional capability, and what remains an alpha limit: the alpha ships local
chat and one guarded Gmail draft flow, not the full assistant experience.
