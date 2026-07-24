## Context

AD-150 requires owner-only gated export/restore over SQLite, blobs, and key material as one atomic snapshot. AD-139 owns the stopped-process set; AD-140 makes per-counterparty deletion terminal; AD-070/071 own compatibility after a base-version change.

The independent stores share no cross-store transaction. `artifacts.d/` and database metadata may be plaintext even though payload blobs, credentials, and scope keys are encrypted. Replacing live files is unsafe. Restoring an older generation also creates a rollback path around terminal erasure unless deletion state has continuity outside restorable generations.

## Goals / Non-Goals

**Goals:**

- Non-delegable root-owner request actions.
- One canonical data-root identity and an exclusive lifetime lock before operation/store access.
- An exact authenticated typed directory tree published atomically with restrictive modes.
- Copy-time validation, monotonic erasure carry-forward, recoverable replacement/rollback, and finalization after the complete startup contract.

**Non-Goals:**

- Hot backup, arbitrary host paths, cloud transport, compression, retention, key escrow, or a remote monotonic-counter service.
- A generic filesystem tool, silent compatibility repair, or encryption of currently plaintext at-rest metadata/YAML.

## Decisions

### 1. Canonical storage identity owns the lifetime lock and snapshot root

Startup creates `data_dir` if absent, rejects a symlink/non-directory root, canonicalizes it once, and uses that canonical path for every store. It derives one adjacent `0700` control directory from the canonical parent plus a collision-resistant digest of the canonical data-root bytes. The process opens its lock file, calls the standard-library exclusive lock API, and retains the guard through shutdown. Pending operations are inspected only after locking and before stores open. Alias/symlink configurations therefore converge on one physical identity and one lock; a second process fails before I/O.

The control directory owns `snapshots/`, pending-operation state, terminal-erasure state, and request-id staging/rollback paths. It is never replaced by restore.

### 2. Actions use bounded bundle names, not caller-selected host paths

`openspine.overlay.export` and `.restore` are registered only in verified owner-control composition and catalogued non-delegable. Worker mint/commission rejects them. Handlers require a canonical sealed root grant (`parent_grant_id == None`, `root_grant_id == id`, and exactly one root chain step with no parent), configured owner principal, and exact effective authority.

Payload is exactly `{ "bundle_name": <1..128 ASCII [A-Za-z0-9_-], not dot-prefixed> }`. The kernel derives `control/snapshots/<bundle_name>`; actions cannot name another path. Export requires that entry absent. Restore requires a pre-staged bundle directory entry that is neither a symlink nor special file. The signed request stores the concrete bundle name plus operation, request id, action id, owner principal id, grant id, and timestamp; audit uses only a digest of the derived path. Pre-open execution revalidates the protected snapshot root, entry type, name, and existence rules under the lifetime lock.

Requests are master-key-HMAC authenticated and written temp → file fsync → rename → control-directory fsync. Only one may be pending. Handlers return `restart_required` and never copy/replace open storage.

### 3. Manifest authenticates the exact typed tree

The manifest body contains version/request metadata and a sorted unique sequence of typed entries:

- directory: normalized relative path;
- regular file: normalized relative path, byte length, SHA-256.

Every directory and file under `data/` appears exactly once; root plus declared ancestors are the only accepted directories. Empty/unlisted directories, duplicates, wrong types, symlinks, special files, absolute/parent/non-normal components, and entries outside `data/` fail validation. Tombstones must be regular files. Directories are forced to `0700`, files to `0600`; mode bits need not be trusted from source. The canonical body has a master-key HMAC.

Export copies to a unique temporary bundle while hashing bytes written, enumerates the completed typed tree for bijection, fsyncs bottom-up, writes/fsyncs the manifest, then renames once inside protected `snapshots/` and fsyncs that directory. A partial final bundle is never visible. The external master key is not copied. The raw at-rest representation remains sensitive.

### 4. Restore validates bytes while copying and installs only staging

Restore verifies manifest schema/HMAC/typed set, then opens each source without following symlinks and copies into same-filesystem staging while hashing/counting. It verifies the completed staged typed tree again. After this point it never reads source bundle bytes.

Replacement uses request-id paths inside the canonical control/staging area and canonical data-root parent:

- restore-new candidate;
- restore-old retained generation;
- restore-rejected candidate after rollback.

Signed stages `requested → staged → installed → finalizing` and `rollback_requested → rolled_back` plus parent fsync make every rename combination idempotently recoverable. The old generation remains until full startup finalization. Invalid/pre-install failures never move active data.

### 5. Signed terminal-erasure continuity remains outside generations

The control directory carries a canonical HMAC-authenticated continuity id, monotonic sequence, and set of erased counterparty ids. The random continuity id is created only with the ledger's first initialization and never changes along that ledger lineage. Erasure durably appends the id and closes the in-process scope BEFORE generation-local database invalidation/key deletion. Failure after that point remains fail-closed; normal startup reconciles the ledger into DB closure, learned/runtime invalidation, audit evidence, key deletion, and regular-file tombstones before serving.

Each export embeds the current signed ledger continuity id/snapshot in its authenticated manifest, but portable recovery additionally requires the latest separately preserved signed ledger, analogous to the external master key. Restore requires the local/imported ledger and bundle baseline to share the same continuity id, then merges their monotonic state; a different freshly initialized ledger fails even when both sequences are zero. Same-host restore automatically has the lineage. The drill requires installing the separately preserved source ledger before fresh-host restore.

Pre-open restore reads no SQLite/ArtifactStore. It applies the merged terminal set to validated staging by deleting scope-key files/aliases and writing/fsyncing regular tombstone files, then enumerates the final typed tree against the authenticated manifest plus this deterministic ledger-authorized delta. Normal startup performs DB/audit reconciliation. This prevents an older snapshot from resurrecting later-erased keys within the stated continuity contract.

A remote anti-rollback counter is out of scope. A malicious host operator deliberately supplying both an old bundle and stale valid ledger is outside the existing owner/host trust boundary; accidental and ordinary portable recovery must preserve the separately managed latest ledger.

### 6. Authorization and finalization survive database replacement

The original action gate audit is in the generation restore replaces. Therefore the signed marker binds digest-safe authorization evidence. After the restored chain verifies, finalization appends idempotent `overlay.restore_requested` (action, owner principal, grant, request, timestamp, path digest) and `overlay.restore_completed`. Export and rollback append equivalent completion events. No plaintext, bundle contents, or key bytes enter audit.

Pending state is threaded through all normal migration, clock, owner, audit, erasure, overlay, registry/persona/model/provider/connector checks, listener bind, and post-bind clock commit. Finalization runs immediately before serving. Only then are old/rejected data and the marker removed. Audit/cleanup failure remains retryable without recopy/reinstall.

### 7. Offline rollback is authenticated and pathless

If an installed restore fails startup, `openspine --rollback-pending-restore` acquires the same lifetime lock, verifies the one signed pending restore, and accepts no directory argument. It durably marks rollback-requested, moves rejected active data aside, reinstalls retained old data with parent fsync, then runs normal startup. After the full contract passes, the old chain receives `overlay.restore_rolled_back`; rejected data and marker are removed. Crash recovery covers each rollback rename boundary. Automatic rollback is rejected because it could hide tampering/incompatibility.

## Risks / Trade-offs

- Restart and local operator access are deliberate consistency/recovery friction.
- Directory bundles need deployment-level copying rather than a built-in download/archive parser.
- Portable restore requires two external prerequisites: the master key and latest signed erasure ledger. Without ledger continuity, restore fails rather than weakening terminal deletion.
- Large exports extend startup and need bundle-sized free space.
- Fixed modes may narrow pre-existing modes, which is safe for runtime data.
- Host-root compromise and a malicious owner intentionally rolling back all external recovery anchors remain outside the current self-hosted trust model.
