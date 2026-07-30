# Add direct terminal chat with Onyx LFM2.5 inference

## Why

OpenSpine's implemented interactive owner surface depends on Telegram.
Local installation, model evaluation, and runtime smoke testing should be
possible on one machine without creating a bot or enabling an external
messaging connector.

Onyx can manage the local inference provider while OpenSpine keeps access
behind its governed model gateway. LFM2.5 provides small local models suitable
for establishing the first functional terminal baseline.

## Change

- Add `openspine chat` and `openspine chat --once <message>`.
- Route local messages through event verification, identity resolution,
  deterministic routing, authority composition, signed task grants,
  contained shell execution, the model gateway, action gating, artifact
  storage, and audit.
- Add a terminal-specific agent, workflow, capability pack, prompt template,
  output channel, and gated reply action.
- Add an `onyx` provider using the normal non-streaming Onyx chat API and a
  Personal Access Token scoped to `write:chat`.
- Configure `LiquidAI/LFM2.5-1.2B-Instruct` by default and
  `LiquidAI/LFM2.5-350M` as the smaller registered alternative.
- Resolve the contained shell executable before clearing the child
  environment, without passing provider credentials or ambient `PATH` into
  the worker.
- Scope conversation continuity by owner channel and workflow rather than a
  single one-shot task grant.
- Keep the repository OpenSpec check portable across development machines
  and CI runners.

## Security boundary

The local CLI owner proof is minted only by the kernel's `chat` command and
cannot be supplied by a shell or connector payload. Terminal authority is
narrower than the Telegram owner-control pack. Model requests still pass
through the kernel-owned provider pool, and the Onyx PAT is never exposed to
the contained shell.

## Non-goals

- No Telegram removal or behavior change.
- No full-screen terminal UI dependency.
- No use of Onyx tools, search, or citations from this provider path.
- No automatic model failover; the 350M model is registered for explicit
  selection and later routing work.
