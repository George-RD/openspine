# Telegram owner-control setup (Phase 1)

How to stand up the Telegram owner-control channel (`implement-telegram-owner-control-slice`)
for local/dev use. This is the Phase 1 slice — one owner, one Telegram bot,
no other channels yet.

## 1. Create the Telegram bot

1. Talk to [`@BotFather`](https://t.me/BotFather) on Telegram.
2. `/newbot`, follow the prompts. BotFather gives you a token that looks
   like `123456789:AAExampleTokenDoNotCommitThis`.
3. **Do not put this token in `openspine.yaml` or anywhere in the repo.**
   It is read from the environment only:

   ```sh
   export OPENSPINE_TELEGRAM_BOT_TOKEN="123456789:AAExampleTokenDoNotCommitThis"
   ```

## 2. Find your Telegram user ID — owner identity, verified structurally

Identity is not authority (D-006): `openspine.yaml`'s `owner.telegram_user_id`
is what the kernel checks every inbound message's sender against
(`telegram.rs::verify_update`). This check has two parts, both required —
neither alone is sufficient:

1. **Sender ID match** — `update.sender_user_id == owner.telegram_user_id`.
2. **Private chat** — `update.is_private_chat` (Telegram's `chat.id ==
   sender.user_id`, i.e. a 1:1 DM, not a group/supergroup/channel). This
   matters even when a message *does* come from the owner: if the owner
   posts in a group they belong to, treating that as owner-control would
   leak Lyra's reply to everyone else in the group. See
   `telegram.rs`'s `owner_message_in_a_group_chat_is_ignored_not_routed`
   test.

Only a message satisfying both is normalized into a `telegram.owner.message`
event with `verified_source: true`; anything else is logged, audited, and
silently ignored (no reply) — PRD §19's "ignore or low-authority triage",
the conservative choice, since Phase 1 has no lower-authority path to
triage into yet.

To find your own numeric Telegram user ID, message
[`@userinfobot`](https://t.me/userinfobot) (or any similar "what's my ID"
bot) from the Telegram account you want to be the owner. Put the number
(not your `@username`) in `openspine.yaml`:

```yaml
owner:
  telegram_user_id: 123456789
  display_name: "Your Name"
```

## 3. Generate the artifact encryption key

Every private payload (a raw Telegram message, an email body, a model
prompt/output, a draft body) is stored encrypted at rest (AES-256-GCM,
`artifact_store.rs`) — never as plaintext. The key is 32 random bytes,
hex-encoded, from the environment:

```sh
export OPENSPINE_ARTIFACT_KEY="$(openssl rand -hex 32)"
```

Must be exactly 64 lowercase hex characters. Losing this key makes every
previously-stored artifact permanently unreadable — back it up like any
other production secret once this leaves dev.

## 4. Minimal `openspine.yaml`

```yaml
data_dir: ./data
sandbox:
  driver: process   # dev/testing only — see "Unsafe dev shortcuts" below
owner:
  telegram_user_id: 123456789
  display_name: "Your Name"
providers:
  - id: anthropic
    kind: anthropic
    model: claude-sonnet-4-20250514   # pick a real, currently-supported model id
    auth:
      mode: api_key
      env: ANTHROPIC_API_KEY
kernel:
  bind_addr: "127.0.0.1:7777"
# lyra_dir defaults to ./artifacts/lyra (this repo's fixtures); override only
# if you're loading a different artifact registry.
```

Then, with `OPENSPINE_TELEGRAM_BOT_TOKEN`, `OPENSPINE_ARTIFACT_KEY`, and
`ANTHROPIC_API_KEY` (or whichever env var your configured provider names)
all set:

```sh
cargo run -p openspine-kernel -- --config openspine.yaml
```

DM your bot on Telegram from the owner account. It replies through
`main_assistant_agent` via the `owner_control_conversation` workflow.

## 5. Unsafe dev shortcuts (do not carry into production)

- **`sandbox.driver: process`** spawns the `openspine-shell` binary as a
  plain child process with a cleared environment — no network or
  filesystem isolation beyond the OS user boundary (see
  `sandbox.rs`'s `ProcessDriver` doc comment). It is explicitly flagged
  unsafe for real private data. Real deployments use `driver: docker`
  (`DockerDriver`: per-task container, internal-only network, non-root,
  read-only rootfs).
- **`unsafe_allow_uncontained_private_data: true`** (default `false`) is
  the *only* way to let the kernel route an `external_communication`-lane
  event (e.g. Phase 2/3's email drafting) through the `process` driver
  (D-025/O-003, PRD §16). Leaving this `false` means the kernel refuses to
  route such events at all until you switch to `driver: docker` — this is
  intentional, not a bug, if you see `route.refused_uncontained` in the
  audit log while testing with `process`. Phase 1's Telegram slice is
  `owner_control` lane, not `external_communication`, so this never
  triggers for the flows this document covers.
- **`kernel.advertise_endpoint`** — only relevant once you switch to
  `driver: docker`; see `docs/kernel-http-contract.md`'s Environment
  section (D-035). Not needed for the `process`/dev setup above.
- Secret intake here (plain environment variables) is a documented Phase 1
  shortcut, not the final design — a richer secret-intake flow
  (`vault.secret_read`, referenced by the capability pack's denied-actions
  list as something the *agent* may never do directly) is future work, not
  fabricated ahead of a real need.
