# Design: Harden approval and budgets

## WYSIWYS (D-045)

```text
draft preview text (full)
  → truncate_for_telegram(full)
  → text == full?
      yes → propose_draft_creation(full)  [unchanged]
      no  → truncate_with_notice(full), send plain reply, no approval proposed
```

Rejected alternative: split the preview across multiple Telegram messages
with the approval button on the last one. Rejected because it creates
drift risk between what the owner reads across parts and what the single
approval record binds — the whole point of digest-bound approval is that
"what was shown" and "what was approved" can never diverge.

## Budget enforcement placement (D-046)

Enforced kernel-dispatch-side, not inside `gate()` — the same placement
precedent as selection-token single-use consumption
(`openspine-gate/src/gate.rs`'s `GateContext` doc comment explains why
`gate()` stays a pure decision function with no side effects of its
own). `max_model_calls` is checked by counting `role = "user"` rows in
`conversation_state` before the new turn is appended, so a limit of `N`
allows exactly `N` calls. `max_artifacts` is checked with one atomic
`INSERT ... ON CONFLICT ... DO UPDATE ... WHERE` statement against a new
`grant_counters` table, mirroring `try_consume_selection_token`'s
TOCTOU-avoidance shape.

The artifact budget counts only shell-initiated puts: the
`model.generate` payload snapshot and the draft-proposal payload. It
does not count internal kernel bookkeeping blobs (conversation turns,
the pending-message ref) — those would otherwise collide with the
default `max_artifacts: 20` limit on ordinary use.

## Task-token hashing and sweep (D-047)

`task_grants.task_token` stores `sha256:<hex>` of the bearer token
(reusing the same raw-bytes digest helper the artifact store uses for
content addressing), not the plaintext. The column name is unchanged —
a rename requires a full SQLite table rebuild for no behavioural
benefit — but its content's meaning changes, documented at the schema
and struct level. `grant_json` embeds a clone of the grant with
`task_token` blanked before serialization, so the raw token cannot be
recovered from either column.

Every call site that reads `.task_token` off a grant does so on a
freshly-minted, in-memory grant (pre-persist) — never on a value loaded
back from the store. Lookup-by-hash (`find_task_grant_by_token`) is the
only authentication path for a persisted grant; nothing needs the
plaintext back.

Expired grants are swept (`DELETE ... WHERE expires_at < now - 24h`) at
the top of `insert_task_grant` — no separate scheduled job exists yet,
so every new grant insert is itself a sweep trigger. 24 hours is
comfortably past the ≤180s task-grant/approval TTLs already in use
elsewhere, so nothing live is ever at risk.

## Trusted notification audit (D-046 continued)

`notify_owner_best_effort` stays ungated (it is kernel-authored courtesy
text to the grant-bound owner chat; gating the trusted kernel against
itself adds ceremony, not security) but every send now appends a
best-effort `owner.notified` audit row, so the trusted-path carve-out
remains traceable from the audit log alone.
