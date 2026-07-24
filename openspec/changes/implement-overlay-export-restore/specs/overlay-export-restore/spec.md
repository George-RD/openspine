## ADDED Requirements

### Requirement: Export and restore are non-delegable root-owner actions

The kernel MUST expose export and restore through the normal gate/audit path and accept them only from an authenticated canonical sealed root task grant whose principal is the configured owner, whose `parent_grant_id` is absent, whose `root_grant_id` equals its id, whose chain contains exactly its parentless root step and no delegation hop, and whose composed authority contains the exact action. Both actions MUST be catalogued as non-delegable and rejected during worker-grant construction. Payload MUST contain only a bounded bundle name, and the kernel MUST derive its location inside a protected snapshot root; callers MUST NOT choose a host path. A successful action MUST durably stage a signed restart-bound operation and MUST NOT copy or replace open storage.

#### Scenario: Verified root owner schedules an export
- **WHEN** the verified owner invokes `openspine.overlay.export` with an allowed root grant and a valid unused bundle name
- **THEN** the action is gated/audited, one signed operation is staged, and the response requires restart

#### Scenario: Owner-derived worker remains unable to schedule
- **WHEN** an owner-derived worker grant requests export or restore even if its parent contains the action
- **THEN** grant construction or the handler rejects it before operation state changes

#### Scenario: Caller-selected path is rejected structurally
- **WHEN** a payload contains a path, dot-prefixed/traversal name, unknown field, or name outside the bounded alphabet/length
- **THEN** strict payload validation rejects it and no filesystem location is touched

### Requirement: Canonical storage identity and exclusive lock enforce stopped-process operation

Startup MUST reject a symlink/non-directory data root, canonicalize one physical data-root identity, use it for every store, derive one adjacent protected control/snapshot root from that identity, and acquire one exclusive operating-system lock before pending-operation inspection or store open. The lock MUST be held for the process lifetime. A signed marker with invalid HMAC MUST fail startup before operation/store I/O.

#### Scenario: Alias cannot bypass process exclusion
- **WHEN** two configurations name the same physical data directory through different aliases
- **THEN** they resolve to the same lock identity and the second process refuses startup while the first holds it

#### Scenario: Forged marker is rejected
- **WHEN** startup finds an operation record whose master-key HMAC does not verify
- **THEN** startup fails closed before opening or modifying a runtime store

### Requirement: Export authenticates and atomically publishes the exact typed tree

A bundle manifest MUST authenticate format/request metadata, a signed terminal-erasure-ledger snapshot, and a sorted bijection with every typed entry under `data/`: each directory exactly once and each regular file exactly once with byte length/SHA-256. Only normalized declared directories and files are permitted. Empty/unlisted directories, duplicate/missing/extra paths, wrong types, symlinks, special files, and non-normal paths MUST fail. Directories MUST be `0700`, files `0600`, and tombstones regular files. The external master key MUST NOT be copied; existing at-rest plaintext metadata/YAML makes the bundle sensitive.

Export MUST copy/hash into a temporary directory under the protected snapshot root, verify the completed typed tree, fsync files/directories/manifest, and publish with one rename plus parent fsync. A pre-existing final bundle MUST not be replaced.

#### Scenario: Complete typed bundle is published atomically
- **WHEN** a pending export runs under the lifetime lock before stores open
- **THEN** only a complete fsynced HMAC-authenticated exact typed tree becomes visible at its final bundle name

#### Scenario: Unlisted empty tombstone directory is rejected
- **WHEN** a bundle gains an empty `keys/<id>.erased/` directory or any other undeclared directory
- **THEN** exact typed-tree validation fails before restore staging or key-ring admission

#### Scenario: Master key stays external
- **WHEN** export completes
- **THEN** the bundle includes wrapped scope keys and the signed erasure baseline but excludes artifact-master-key bytes

### Requirement: Restore installs only bytes validated while copying

Restore MUST revalidate the protected snapshot entry named in the signed record. It MUST verify manifest schema/HMAC/typed set, copy each no-follow source file into same-filesystem staging while hashing/counting, and verify the completed staged typed tree. After staged validation the installer MUST never reread source bundle bytes. Signed request-id stages and new/old/rejected paths MUST make every directory rename/fsync crash-recoverable and idempotent. Invalid/pre-install failures MUST leave active data untouched; the old generation MUST remain through full finalization.

#### Scenario: Source changes during copy
- **WHEN** a bundle file or directory tree changes after initial verification or while copied
- **THEN** copy-time hashing or staged exact-tree validation fails and no candidate is installed

#### Scenario: Crash between generation renames is recovered
- **WHEN** the process stops after moving old data but before or after installing staging
- **THEN** signed stage plus request-id paths complete the same transition at most once without mixing generations

### Requirement: Signed erasure continuity is merged and reconciled before serving

The protected control root MUST maintain an HMAC-authenticated random continuity id, monotonic sequence, and erased-counterparty set outside replaceable generations. The continuity id MUST be created only on first initialization and preserved through updates/export/import. Erasure MUST durably add the id and close its in-process scope before generation-local invalidation/key deletion. Every export MUST embed an authenticated ledger baseline. Restore MUST require the destination/local-imported ledger and bundle baseline to share the same continuity id, then merge non-regressing state. A fresh unrelated ledger MUST fail even at sequence zero. Same-host restore uses its live lineage; portable restore requires the separately preserved latest source ledger.

Pre-open restore MUST use only signed ledger state to remove staged wrapped keys/aliases and create/fsync regular tombstones, then validate the final staged typed tree against the manifest plus that deterministic authorized delta. Normal startup MUST reconcile ledger scopes into database closure, learned/runtime invalidation, audit evidence, key/tombstone state, and no key regeneration before serving.

#### Scenario: Pre-erasure snapshot cannot resurrect deleted payloads
- **WHEN** a scope is erased after export and an older bundle is restored with the same-host or separately preserved latest ledger
- **THEN** the later erasure is applied to staging and startup, so the restored key never loads and ciphertext remains unrecoverable

#### Scenario: Fresh host lacks continuity
- **WHEN** a portable restore is attempted without an established signed ledger at least as new as the bundle baseline
- **THEN** restore fails before installing the bundle

### Requirement: Full startup and authorization replay precede finalization

Pending operation state MUST survive all normal migration, clock, owner, audit, erasure, overlay compatibility/admission, registry/persona/model/provider/connector checks, listener bind, and post-bind clock commit. Finalization MUST run immediately before serving. The signed restore record MUST bind operation, bundle name, action id, owner principal id, grant id, request id, and timestamp. The restored chain MUST receive idempotent digest-safe restore-requested authorization evidence followed by completion before old data/control state is removed. Export/rollback completion MUST follow the same plaintext-free audit-before-cleanup rule. Failure MUST remain retryable without recopy/reinstall.

#### Scenario: Older snapshot runs compatibility pass on newer base
- **WHEN** a valid older-base bundle is restored under a newer compatible binary
- **THEN** compatible overlays load and orphaned learned artifacts enter reconfirmation before finalization

#### Scenario: Late startup failure retains recovery state
- **WHEN** provider validation, listener bind, or post-bind clock commit fails after installation
- **THEN** the signed request and old generation remain and no completion is claimed

### Requirement: Failed restore has authenticated offline rollback and a proven drill

`openspine --rollback-pending-restore` MUST acquire the lifetime lock, verify the one signed pending restore, accept no replacement path, durably transition rollback stages, move rejected data aside, reinstall retained old data, and enter normal startup. Only after full startup and an idempotent rollback audit MAY rejected data/control state be removed. Every rollback rename boundary MUST recover idempotently.

The operator guide and production-path tests MUST cover gated request/restart, lock behavior, sensitive bundle transfer under the protected snapshot root, separate master-key and latest-ledger preservation, exact validation, terminal-erasure carry-forward, newer-base compatibility, late failure, and rollback.

#### Scenario: Validation-failed restore rolls back
- **WHEN** an installed generation cannot complete startup and the operator invokes rollback with matching external state
- **THEN** old data becomes active, survives rollback crash boundaries, passes startup, records rollback, and does not reinstall rejected data
