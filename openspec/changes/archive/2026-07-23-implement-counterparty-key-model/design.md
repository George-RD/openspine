# Design: implement-counterparty-key-model

## Data model

### Per-counterparty keys (`counterparty_keys.rs`)

```
<data_dir>/keys/<ulid> := [ "OSK1" ][ nonce:12 ][ AES-256-GCM ciphertext ]
AAD                     := "openspine.counterparty_key.v1" || ulid
```

- The kernel master key wraps and unwraps random 32-byte payload keys; artifact
  payload bytes are encrypted only with the payload key for their scope.
- `CounterpartyKeyRing::get_or_create_key(id)` lazily creates one key per
  scope. Publication uses a synced temporary file and hard-link installation,
  followed by mandatory alias cleanup and directory synchronization.
- Current key files authenticate both a domain label and the counterparty id.
  Legacy unversioned key files remain readable and migrate to `OSK1` after a
  successful unwrap.
- `CounterpartyKeyRing::open` removes orphaned temporary files and, for each
  existing `<ulid>.erased` tombstone, completes deletion of any stale key file
  left by a crash.
- `erase(id)` rejects `SYSTEM_SCOPE`. For every other id it durably creates an
  empty `<ulid>.erased` tombstone, removes the wrapped key and any temporary
  aliases, and synchronizes the key directory. It returns whether a key file
  existed, but even a never-used scope is permanently closed.
- `get_key(id)` returns `Ok(None)` for an absent or erased key.
  `get_or_create_key(id)` returns `Erased` once the tombstone exists. No
  plaintext payload key is cached between calls.

### Blob format (`artifact_store.rs`)

```
<data_dir>/artifacts/<scope-ulid>/<sha256-hex> :=
    [ tag=3:1 ][ scope:16 ][ nonce:12 ][ AES-256-GCM ciphertext ]

AAD := [ tag=3:1 ][ scope:16 ]
```

- Blobs are content-addressed by `(scope, digest)`. Identical plaintext in two
  scopes produces the same digest-only `ArtifactRef` but two separately keyed
  files. Scope is passed separately; no scope field is added to `ArtifactRef`.
- The plaintext header retains attribution after crypto-erasure, while format
  3 authenticates the tag and scope as associated data. Copying a blob to a
  different scope and rewriting its header cannot make it decrypt there.
- `put(plaintext)` delegates to `put_scoped(SYSTEM_SCOPE, plaintext)`;
  `get(ref)` delegates to `get_scoped(SYSTEM_SCOPE, ref)`. Existing callers
  therefore remain on the reserved internal/legacy scope.
- `put_scoped(scope, plaintext)` holds the scope lock through key access and
  durable publication. An existing `(scope, digest)` is resynchronized before
  retry success. An erased scope rejects the write instead of creating a fresh
  key.
- `get_scoped(scope, ref)` holds the same scope lock through file read, key
  unwrap, AEAD decryption, digest verification, and return. The requested
  scope must match the header.
- `scope_of(scope, ref)` reads and validates the plaintext scope header without
  key access, so attribution remains available after erasure.
- Recovered format 2 (`[tag=2][scope][nonce][ciphertext]`, without associated
  data) remains readable. A successful read verifies the digest and rewrites
  the blob in format 3 under the same `(scope, digest)` path.

### Legacy migration (`ArtifactStore::migrate_legacy_blobs`)

On startup, each pre-AD-140 flat format-1 blob
`[nonce:12][ciphertext]` is identified by successful decryption with the
legacy master key and digest verification, not by its first byte. It is
re-encrypted as format 3 under `SYSTEM_SCOPE` at
`<SYSTEM_SCOPE>/<sha256-hex>`. The target is written and synchronized before
the flat source is removed. If a crash leaves both paths, the next run verifies
the scoped target before deleting the source. The migration is idempotent.

### Derived-artifact invalidation (`store/learned_artifacts.rs`)

- `Provenance::ProducedBy` records `source_scope` at production time. Erasure
  matches this field directly; digest-only matching is ambiguous when two
  scopes contain identical plaintext. Legacy/internal producers may use
  `SYSTEM_SCOPE`.
- `CompatibilityStatus::Erased` is terminal. The erasure transaction consumes
  linked action requests and clears pending reconfirmation fields. Later
  reconfirmation transitions require the expected non-erased source state.
- The `erased_counterparties` row is the durable closure authority. Database
  triggers reject every later `ProducedBy` insert or replacement for that
  scope, closing the blob-write-to-metadata race.
- `Store::mark_learned_artifacts_erased` holds the artifact store's scope lock
  and one immediate SQL transaction while it resolves exact
  `(kind, artifact_id, version)` identities, marks them `erased`, cancels
  reconfirmation, inserts the closure marker, and appends the audit event.
- The audit event binds `counterparty:<ulid>` and digest references for the
  exact invalidated identities; it contains no learned payload plaintext.
  Repeated erasure is gated by the database marker and does not duplicate the
  audit event.

### Erasure and startup reconciliation (`counterparty_erasure.rs`, `main.rs`)

`erase_counterparty(store, artifacts, id)` rejects `SYSTEM_SCOPE`, performs the
transactional invalidation/audit/closure operation, then durably tombstones the
scope and deletes its wrapped key. The returned report includes the exact
invalidated identities for later owner-facing caller adoption.

The database transaction commits before filesystem deletion. If a crash occurs
between those steps, startup enumerates `Store::erased_counterparty_ids()` and
idempotently calls `ArtifactStore::erase_counterparty_key` before serving,
recreating the tombstone and completing key deletion. Existing filesystem
tombstones are also reconciled when the key ring opens.
## Why keys are files, not a SQL table

AD-139 enumerates the backup/restore snapshot set as "SQLite DB + artifact
blobs + keys" — three distinct elements. A sibling `keys/` tree keeps that
split literal. SQLite stores only the durable scope-closure marker used to
reconcile an interrupted erasure; it never stores key material.
## Test strategy

Focused tests cover:
- key creation/persistence, distinct scopes, idempotent permanent erasure,
  reserved `SYSTEM_SCOPE` rejection, associated-data key substitution,
  legacy-key migration, tombstone recovery, temporary-alias cleanup, and
  durability retries;
- format-3 scoped round trips, cross-scope isolation, erased payload
  unreadability, permanent write rejection, header attribution, scope/header
  substitution rejection, format-2 recovery, flat format-1 migration, and
  migration crash recovery;
- exact provenance invalidation and audit references, audit-chain survival,
  repeated erasure, reserved-scope rejection, terminal stale reconfirmation,
  post-closure metadata rejection, and startup exclusion of erased artifacts.
