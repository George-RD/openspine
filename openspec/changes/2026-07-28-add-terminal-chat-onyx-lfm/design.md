# Design: direct terminal chat with Onyx LFM2.5

## Decisions

### 1. Treat the terminal as a distinct verified owner channel

`openspine chat` mints `cli.owner.message` events with `local_cli_auth` and
`owner_device` channel trust. The terminal receives a dedicated route,
agent, workflow, capability pack, output channel, and reply action. This
avoids borrowing Telegram authority merely because both surfaces serve the
owner.

### 2. Keep the existing kernel and shell boundary

Each terminal turn creates a signed task grant and runs the existing
contained shell. The shell requests model generation through the kernel and
returns text only through `terminal.reply:owner_device`. An in-process
channel carries that gated result back to the terminal loop.

The process driver resolves the `openspine-shell` executable before clearing
the worker environment. The child still receives only the task token and
kernel endpoint; provider credentials and the parent `PATH` are not inherited.

### 3. Use Onyx through its scoped chat API

OpenSpine implements a bounded `onyx` provider client for the non-streaming
`/chat/send-chat-message` endpoint. Authentication uses an Onyx Personal
Access Token scoped to `write:chat`, retained only inside the kernel.

The adapter disables Onyx tools and citations. It sends the current user turn
as the Onyx message and supplies OpenSpine's resolved instructions and prior
conversation as ephemeral `additional_context`. OpenSpine remains the source
of conversation continuity while Onyx owns the configured LFM2.5 provider.

The provider base URL is deployment-specific. The retained example uses
`http://127.0.0.1:8080`; production or container deployments can replace it
without changing the binary.

### 4. Establish a model hierarchy without silent failover

`LiquidAI/LFM2.5-1.2B-Instruct` is first in the provider list and therefore
becomes the initial provider for base, matcher, and miner roles.
`LiquidAI/LFM2.5-350M` is registered as a smaller alternative for simple
tasks and later routing experiments. This change does not silently switch
models after an error.

### 5. Scope conversational history to channel and workflow

Terminal turns intentionally use separate task grants. Recent conversation
lookup therefore binds history to both the persisted owner channel id and
workflow id. This enables multi-turn chat without crossing channel or
workflow boundaries.

## Trade-offs

- A line-oriented REPL is less polished than a full TUI, but it proves the
  governed runtime with no new UI dependency.
- A native Onyx adapter adds a small transport surface, but it works with the
  normal scoped PAT and chat API rather than requiring a separately scoped
  gateway credential.
- Onyx receives the current turn plus ephemeral OpenSpine context, but its own
  prompt layer still participates; this is not byte-for-byte equivalent to a
  direct OpenAI-compatible call.
- Registering two models creates a clean future routing seam, but explicit
  selection remains necessary until model-role routing is extended.
