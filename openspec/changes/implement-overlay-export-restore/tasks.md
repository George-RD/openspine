## 1. Control and bundle primitives

- [x] 1.1 Canonicalize/reject aliased data roots, derive the collision-resistant adjacent `0700` control/snapshot root, and hold its exclusive standard-library lifetime lock before operations/stores through shutdown.
- [x] 1.2 Add strict signed operation and bundle-manifest types with canonical action/owner/grant/bundle-name binding, master-key HMAC verification, and exact unique typed-directory/regular-file trees.
- [x] 1.3 Add no-follow copy-while-hash, fixed `0700`/`0600` modes, bottom-up fsync, atomic publication, and source/staged typed-tree tests covering extras/empty tombstone directories, duplicates, wrong types, symlinks, concurrent mutation, and partial publication.

## 2. Monotonic erasure control state

- [x] 2.1 Add a signed monotonic terminal-erasure continuity id/set/sequence outside replaceable generations, update it before generation-local erasure while closing the in-process scope, and embed its authenticated baseline in every export.
- [x] 2.2 Reconcile ledger scopes into database/audit/runtime invalidation and key/tombstone state during startup before serving, including retry after post-ledger generation-local failure.
- [x] 2.3 Merge same-continuity non-regressing local/imported ledgers, harden and re-enumerate staged typed trees without opening stores, and test old-bundle resurrection plus fresh-host missing/regressed/unrelated-ledger failures.

## 3. Restart-bound export and restore state machines

- [x] 3.1 Implement pre-open export staging/publication and idempotent published-state recovery without copying the external master key.
- [x] 3.2 Implement restore copy-validation, signed request-id-bound stages, same-filesystem new/old generation replacement, and idempotent crash recovery at every rename/fsync boundary.
- [x] 3.3 Retain the old generation and signed operation through the full startup contract; retry completion audit/cleanup without recopying or reinstalling.
- [x] 3.4 Add `--rollback-pending-restore` with authenticated rollback-requested/rolled-back stages, crash-safe rejected/old swaps, post-startup rollback audit, and failpoint tests.

## 4. Authority and owner surface

- [x] 4.1 Register `openspine.overlay.export` and `openspine.overlay.restore` with no egress/output channel and catalog-owned non-delegable classification enforced during worker mint/commission.
- [x] 4.2 Admit the actions only through verified owner-control composition and enforce canonical sealed-root grant structure with no delegation hops, configured owner principal, exact authority, strict one-field bundle-name payload, one pending operation, and protected snapshot-root rules.
- [x] 4.3 Add thin shell `/export <bundle-name>` and `/restore <bundle-name>` submissions plus root-owner success, foreign-principal, owner-derived-worker, malformed name, canonical-alias lock contention, and conflicting request tests.

## 5. Startup, audit, and compatibility

- [x] 5.1 Process signed operations before ArtifactStore/SecretStore/SQLite open and thread pending finalization past all migration, clock, owner, audit, erasure, overlay, registry, model/provider/connector, listener-bind, and post-bind-clock checks.
- [x] 5.2 Replay digest-safe restore authorization into the restored chain and append idempotent export/restore/rollback completion before clearing control state or deleting retained generations.
- [x] 5.3 Add a production-path export→restore test under a newer base epoch proving exact data, permissions, key/tombstone closure, existing orphan/reconfirmation behavior, late-startup retention, and explicit rollback.

## 6. Operator contract and verification

- [x] 6.1 Replace the day-2 snapshot drill with named gated request/restart steps, lock behavior, protected snapshot staging, separate master-key/latest-ledger continuity, sensitive bundle transport, full finalization boundary, failure recovery, and rollback.
- [x] 6.2 Reconcile proposal, design, tasks, and both delta specs with implementation; record deviations, edge cases, and ratified D-122 in IMPLEMENTATION-NOTES.md/decision log.
- [x] 6.3 Run focused operation/authority/recovery tests, format, workspace clippy/tests, file-size/claims/ceremony gates, strict OpenSpec validation, `graphify update .`, and `scripts/check.sh implement-overlay-export-restore`.
