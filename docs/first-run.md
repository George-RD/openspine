# First run: the trust ceremony

`openspine init` is the single command that turns a built binary into a
running, governed install. It is a trust loop, not a repository-setup exercise:
one command establishes one trusted principal and reports the boundary before
you send Lyra any work.

```bash
openspine init --owner <telegram_user_id> --name "Your Name"
```

`--owner` is your Telegram user id — the single trusted principal every later
approval and audit row is bound to. Message
[@userinfobot](https://t.me/userinfobot) to find yours. `--name` is your
display name and defaults to `$USER`. Pass `--config <path>` to place the
configuration somewhere other than `openspine.yaml` in the current directory.

The command is non-interactive and idempotent: re-running it fills in anything
missing and never overwrites key material or the bound owner identity.

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

`init` only writes the principal on a genuinely fresh data root. On an existing
install it leaves the store alone; the kernel validates and re-binds the owner
principal during its normal boot.

### 3. Test

`init` prints a readiness report and the plain-language trust ceremony. A green
report means the configuration, seed keys, and Lyra package are in place. To
close the loop with a governed reply:

```bash
openspine chat
```

Inside chat, `/status` reprints readiness and any message becomes a verified
owner request that runs through the full grant, gate, and audit path. See
[Direct terminal chat](terminal-chat.md) for the model-provider configuration
the first reply needs.

## After the first loop

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
