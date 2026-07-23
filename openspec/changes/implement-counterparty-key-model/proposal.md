# Proposal: implement-counterparty-key-model

## Summary

AD-140 (resolves OQ-7) mandates crypto-erase for counterparty deletion:
private payloads stored in an identity scope are encrypted under that scope's
key, and erasing the counterparty permanently deletes the key while preserving
the audit hash chain. Derived overlay artifacts are invalidated through the
`source_scope` recorded in their D-077 provenance.

This change introduces the versioned key ring and blob formats, the terminal
erasure primitive, and startup recovery. It is the foundation
`implement-overlay-export-restore` depends on (HARD edge: the single-key design
must not be baked into the export format).
## Canonical basis

- **AD-140** Crypto-erase for counterparty deletion (settled) — per-counterparty
  keys, erasure = key deletion, plaintext unrecoverable while hash chain intact,
  derived artifacts invalidated via provenance links.
- **D-077** Learned artifacts carry exchange provenance; reconfirmation records
  a durable anchor — what makes erasure propagation computable.
- **D-078 / D-079** Compatibility-status machinery the `Erased` state extends.
- **AD-139** Backup/restore treats "SQLite DB + artifact blobs + keys" as one
  snapshot set — keys are a first-class, separately-snapshotted element.

## Why now

`implement-counterparty-key-model` is an early leaf of the agent-OS sequence; it
`Requires:` `implement-identity-store-and-principal` (counterparties are
identity ids) and `implement-overlay-model` (provenance links). Both are
archived, so the change is eligible. The export/restore change that follows is
HARD-blocked on it.

## Scope

In scope:
- Per-counterparty payload key ring (`counterparty_keys.rs`): one random
  256-bit payload key per scope, stored in a versioned key file and wrapped
  under the master key with associated data binding the key to its scope.
- `artifact_store.rs` scoped storage at `(scope, digest)`, using blob format 3
  with the format tag and plaintext scope header authenticated as AEAD
  associated data. Recovered format-2 blobs remain readable and migrate to
  format 3 on read.
- Startup migration of pre-AD-140 flat format-1 blobs into format 3 under the
  reserved `SYSTEM_SCOPE`.
- Existing `ArtifactStore::put/get` signatures remain `SYSTEM_SCOPE`
  convenience wrappers. `ArtifactRef` remains digest-only; scoped callers pass
  scope separately to `put_scoped/get_scoped/scope_of`.
- `erase_counterparty` rejects `SYSTEM_SCOPE`, invalidates derived artifacts
  through their recorded `Provenance::ProducedBy.source_scope`, commits a
  durable database closure marker and D-012-safe audit entry, then writes the
  filesystem tombstone and deletes the wrapped key.
- Erasure is a permanent scope closure. Reads, writes, and erasure for one
  scope are serialized; later writes cannot recreate its key.
- `CompatibilityStatus::Erased` is terminal. Erasure consumes and clears
  pending reconfirmations so stale approvals cannot restore an erased
  artifact, and later learned-artifact writes for a closed scope are rejected.
- Startup reconciliation replays durable database closure markers into the
  key ring, completing tombstone creation and key deletion after a crash.
- Audit entries identify the erased counterparty and the exact invalidated
  artifact identities through ULID/digest-safe metadata, never plaintext.

Out of scope (separate changes):
- Owner-facing "delete counterparty" API action and production caller adoption
  (including live-registry eviction using the primitive's returned artifact
  identities).
- Export/restore of key material (AD-150) — depends on this change.
- Re-keying existing production `ArtifactStore::put/get` callers onto
  identity scopes. They continue to use `SYSTEM_SCOPE`; new scoped call sites
  can use the explicit scoped APIs.
## Design implications / risks

- **Versioned at-rest formats.** Current key files are
  `[OSK1][nonce][ciphertext]`, with a domain label and scope as associated
  data. Current blobs are `[tag=3][scope:16][nonce:12][ciphertext]`, with
  `[tag=3][scope]` as associated data. Legacy unversioned key files and
  recovered format-2 blobs migrate in place; flat format-1 blobs migrate at
  startup under `SYSTEM_SCOPE`.
- **Permanent closure.** A durable database marker and empty filesystem
  tombstone prevent a deleted scope from minting another key. Repeated erasure
  is idempotent and emits no duplicate audit row.
- **Content addressing is preserved within a scope.** Identical plaintext has
  the same digest-only `ArtifactRef`, but each scope has a separate encrypted
  blob and key.
- **Crash ordering is recoverable.** Database invalidation, closure marker,
  reconfirmation cancellation, and audit commit before filesystem key
  deletion. Startup reconciles any committed marker whose tombstone/key
  cleanup did not finish.
- **No maintained in-memory key cache.** Scoped reads hold the same per-scope
  lock as erasure through decryption and digest verification, so erasure cannot
  return while a racing read can still return plaintext.
- **Caller boundary remains deliberate.** Existing `put/get` callers and
  `ProducedBy` records for legacy/internal sources use `SYSTEM_SCOPE`.
  `SYSTEM_SCOPE` is valid provenance but is rejected by counterparty erasure.
