# Tasks: implement-counterparty-key-model

## 1. Per-counterparty key ring

- [x] Add `counterparty_keys.rs` with `CounterpartyKeyRing` using one random
  32-byte payload key per scope and versioned `OSK1` key files whose associated
  data binds the domain and scope.
- [x] Add `SYSTEM_SCOPE = Ulid::nil()` for existing `put/get` callers,
  legacy/internal provenance, and migrated flat blobs; reject it from
  counterparty erasure.
- [x] Make erasure permanently close a scope through a durable empty
  filesystem tombstone, physical key/temporary-alias deletion, and no
  maintained plaintext-key cache.
- [x] Read and migrate legacy unversioned key files; reconcile tombstones and
  orphaned temporary files when the key ring opens.
- [x] Cover distinct/persistent keys, wrong-master failure, associated-data
  key substitution, permanent/idempotent erasure, reserved-scope rejection,
  crash recovery, concurrency, and durability retries with focused tests.
## 2. Re-key artifact store to per-counterparty encryption

- [x] Add current blob format 3
  `[tag=3][scope:16][nonce:12][ciphertext]` at
  `<scope-ulid>/<sha256-hex>`, authenticating `[tag=3][scope]` as AEAD
  associated data.
- [x] Keep `ArtifactRef` digest-only and preserve `put/get` signatures as
  `SYSTEM_SCOPE` wrappers.
- [x] Add `put_scoped(scope, plaintext)`, `get_scoped(scope, ref)`, and
  `scope_of(scope, ref)`, with header/path validation and plaintext digest
  re-verification.
- [x] Serialize complete scoped reads and writes with erasure; reject writes
  after permanent scope closure and re-synchronize existing blobs on retry.
- [x] Read recovered format-2 blobs, verify their digest, and rewrite them as
  associated-data-bound format 3.
- [x] Add idempotent, crash-safe migration of pre-AD-140 flat format-1 blobs
  into format 3 under `SYSTEM_SCOPE`.
- [x] Cover existing content-addressing/ciphertext/tamper behavior, per-scope
  isolation, erased-payload unreadability, permanent write rejection,
  header attribution, scope/key substitution rejection, format-2 recovery,
  flat format-1 migration, and migration crash recovery with focused tests.
## 3. Derived-artifact invalidation status

- [x] Add terminal `CompatibilityStatus::Erased` and exclude erased artifacts
  from startup overlay/persona admission.
- [x] Record `Provenance::ProducedBy.source_scope`, backfilling existing typed
  provenance to `SYSTEM_SCOPE`; use the recorded scope in provenance readers.
- [x] Add a durable `erased_counterparties` marker and transaction-time guards
  that reject learned-artifact writes for a closed scope.
- [x] Make erasure consume and clear pending reconfirmation state, and require
  non-erased source states for later compatibility transitions.
- [x] In one immediate transaction, resolve and mark exact artifact identities,
  insert the closure marker, and append a no-plaintext audit event binding the
  counterparty and exact digest-safe target references.
## 4. Erasure orchestrator

- [x] Add `counterparty_erasure.rs` with
  `erase_counterparty(store, artifacts, counterparty_id)`, rejecting
  `SYSTEM_SCOPE`, invalidating by recorded `ProducedBy.source_scope`, then
  deleting the key after the database transaction commits.
- [x] Return the exact invalidated `(kind, artifact_id, version)` identities
  for owner-facing caller adoption without adding that caller in this change.
- [x] Cover payload unreadability, other-scope isolation, audit-chain
  verification, exact audit targets, repeated/keyless erasure, reserved-scope
  rejection, and stale reconfirmation with focused tests.
## 5. Startup wiring

- [x] Register `counterparty_keys` and `counterparty_erasure` in `main.rs`.
- [x] Run flat format-1 blob migration under `SYSTEM_SCOPE` after opening the
  artifact store.
- [x] After opening SQLite, replay every durable erased-counterparty marker
  into the key ring before serving, completing tombstone/key cleanup after a
  database-commit-before-filesystem crash.
## 6. OpenSpec + verification

- [x] Reconcile `proposal.md`, `design.md`, `tasks.md`, and
  `specs/counterparty-key-model/spec.md` with the implemented contracts.
- [x] Verification gate: rerun `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, `bash scripts/check-file-sizes.sh`, and
  `openspec validate implement-counterparty-key-model --strict`.
- [x] Write `IMPLEMENTATION-NOTES.md`.
