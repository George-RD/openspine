# Design

## Canon and audited sites
AD-138 is settled and requires no failed effect without a durable record and owner-visible surface. AD-137 is settled code-audited evidence. AD-082 remains leaning; this change owns only the ordered unresolved digest substrate and leaves briefing/presentation policy to the shell.

The audited fire-and-forget sites were:
- `crates/openspine-kernel/src/api/actions.rs`: dispatch failure and draft proposal-failed audit appends.
- `crates/openspine-kernel/src/api/mod.rs`: authentication rejection audit appends.
- `crates/openspine-kernel/src/pipeline/mod.rs`: owner notification gate outcome and notification outcome.

All action-path appends now propagate `StoreError` as an internal failure. The owner notification path is governed separately: it records the attempt, sends, then durably records either `owner.notified` or atomically records `owner.notify_failed` with a dead-letter row.

## Routing
`Authority` and `Escalation` route to immediate owner notification. `Connector` and `Resource` route to the digest. The pure taxonomy guard rejects placing an immediate class in the digest. Digest writes include an audit receipt in one transaction.

## Storage
`store/failure_surfacing.rs` owns `digest_items`, `notify_dead_letters`, and `connector_counters`. Dead letters have pending/in-progress/resolved state, attempt count, lease, encrypted text reference, and next-attempt timestamp. Artifact persistence is a prerequisite to dead-letter insertion; if it fails, a plaintext-free `owner.dead_letter_persist_failed` audit plus connector-class digest record remains owner-visible and no blank retry is enqueued. `/digest` is recognized only after verified-owner Telegram update handling and returns unresolved items.
Digest detail follows the same encrypted artifact/text-ref convention as the
notification DLQ. New digest writes require a verified artifact ref and keep
only bounded non-sensitive summary metadata plus the ref/hash in SQLite;
`failure.digest_batched` and delivery receipts carry refs, never detail
plaintext. The migration is deliberately fail-closed for legacy rows: after
adding the nullable `text_ref` column, rows with `NULL` refs have their old
summary replaced idempotently with `[<class>] legacy failure detail
unavailable`. `/digest <ULID> [page]` is the minimal secure technical
retrieval substrate: it deterministically emits UTF-8 byte-bounded pages
(`page N/M`) from the full encrypted detail, with a stable ref and truthful
invalid/out-of-range responses. Every byte is recoverable; personality-seed
fold/presentation style remains deferred per AD-082. `failure.digest_detail_viewed`
is appended only after a decrypted page is delivered and carries page metadata;
unavailable legacy/corrupt/missing-key responses carry a distinct
`failure.digest_detail_unavailable` receipt and never `detail_viewed`.

We adopt `/digest <ULID> [page]` as the deterministic, lossless technical
pagination substrate now; defer AD-082 personality/fold wording and
presentation style to the personality-seed work.
The implementation preserves deny-by-default, shell containment,
identity-not-authority, grant-only authority, deterministic routing,
kernel/shell split, and digest-bound approval.
