# Direct terminal chat

`openspine chat` runs Lyra locally without Telegram. It remains inside the
kernel, signed task-grant, contained shell, model gateway, action gate,
artifact store, and audit boundaries used by connector-backed work.

## Build

```bash
cargo build --workspace --bins
export PATH="$PWD/target/debug:$PATH"
```

The process sandbox resolves `openspine-shell` before clearing the worker
environment, so no provider credentials or other ambient variables are passed
to the contained worker.

## Configure Onyx and LFM2.5

Run Onyx at `http://127.0.0.1:8080`, configure its default LLM provider to the
LFM2.5 endpoint, and create a Personal Access Token scoped only to
`write:chat`.

Copy the example configuration:

```bash
cp openspine.terminal.example.yaml openspine.terminal.yaml
```

The example registers `LiquidAI/LFM2.5-1.2B-Instruct` first and
`LiquidAI/LFM2.5-350M` as a smaller alternative. Both use `kind: onyx` and
reference `ONYX_PAT`; the token value stays outside YAML.

```bash
export ONYX_PAT="<onyx-write-chat-pat>"
export OPENSPINE_ARTIFACT_KEY="$(openssl rand -hex 32)"
export OPENSPINE_GRANT_HMAC_KEY="$(openssl rand -hex 32)"
export OPENSPINE_WEBHOOK_HMAC_KEY="$(openssl rand -hex 32)"
```

OpenSpine calls Onyx's non-streaming `/chat/send-chat-message` API. It disables
Onyx tools and citations for this provider path, supplies OpenSpine's resolved
system and conversation context as ephemeral additional context, and keeps the
PAT in the kernel. Onyx creates a chat session for each model call; OpenSpine
remains the source of conversation continuity.

## Run

```bash
openspine --config openspine.terminal.yaml chat
```

Type `/exit`, `/quit`, or press Ctrl-D to stop.

One-message smoke test:

```bash
openspine --config openspine.terminal.yaml chat \
  --once "Reply with exactly OPENSPINE_OK and nothing else."
```

`OPENSPINE_TELEGRAM_BOT_TOKEN` is not required in chat mode.

## Authority boundary

Terminal grants permit status, setup, approved model generation, and
`terminal.reply:owner_device`. They do not grant Telegram reply authority or
the broader Telegram owner-control tool set.
