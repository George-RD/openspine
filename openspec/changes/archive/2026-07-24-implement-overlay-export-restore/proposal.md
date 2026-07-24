## Why

The learned overlay is the durable record of the owner's relationships and operating preferences, but the current stopped-process backup procedure has no first-class, gate-mediated way to export or restore it as a coherent unit. AD-150 now becomes implementable because AD-140's per-counterparty key model has landed, so the export format can preserve crypto-erasure rather than freezing the former single-key design.

## What Changes

- Add non-delegable root-owner `openspine.overlay.export` and `openspine.overlay.restore` actions through the normal kernel gate and audit path, addressing bundles only by bounded names inside a kernel-controlled snapshot root.
- Stage gated requests for execution before storage opens on the next process start, under an exclusive lock derived from one canonical data-directory identity.
- Define an atomically published, versioned directory bundle with a canonical exact typed-tree manifest, fixed restrictive permissions, and a master-key HMAC; the external `OPENSPINE_ARTIFACT_KEY` and latest signed terminal-erasure continuity ledger are preserved separately.
- Restore by copy-and-hash validation into a sibling staging directory, merging the external monotonic erasure ledger, performing a crash-recoverable data-directory swap, then running the complete normal startup and AD-070/071 overlay compatibility path before finalization.
- Add a documented export/restore drill and an end-to-end round-trip test that restores an older-base snapshot under a newer base epoch and surfaces incompatible learned artifacts through the existing reconfirmation path.

This changes OpenSpine core system operations and private-data handling. It does not add external communication or connector access, and it does not weaken runtime authority.

## Capabilities

### New Capabilities

- `overlay-export-restore`: Owner-gated, restart-bound atomic export and restore of the complete OpenSpine at-rest data representation with authenticated manifests and post-restore compatibility handling.

### Modified Capabilities

- `day-2-operations`: Replace the existing one-set snapshot requirement with the first-class gated, restart-bound bundle and restore contract, including `keys/`, external master-key/erasure-ledger continuity, restrictive permissions, rollback, and full startup finalization.

## Impact

- **Affected code:** action catalog/handler registration, owner-control capability pack, startup sequencing, a new export/restore module, and focused operation/recovery tests.
- **Affected data:** the complete configured `data_dir`, including `kernel.db`, `artifacts/`, `keys/`, `credentials/`, and `artifacts.d/`; the artifact master key and latest signed terminal-erasure ledger remain external recovery prerequisites.
- **Authority:** both request actions are non-delegable, require a root verified-owner grant, and pass through `gate()`; signed markers bind digest-safe authorization evidence for replay into a restored audit chain.
- **Operations:** every process holds an exclusive data-directory lifetime lock; completing either operation requires a controlled restart, and a failed restore has an authenticated offline rollback transition.

## Non-goals

- No cloud destination, download transport, arbitrary host path, compression, scheduling, retention policy, or remote key/monotonic-counter service.
- No live/hot snapshot claim and no replacement of SQLite or filesystem stores while they are open.
- No inclusion of `OPENSPINE_ARTIFACT_KEY` in the exported bundle.
- The bundle preserves existing at-rest representations and therefore may contain plaintext SQLite metadata or overlay YAML; it requires secure local handling.
- No silent base/overlay compatibility repair, automatic owner reconfirmation, or bypass of existing crypto-erasure tombstones.
- No general host-filesystem read/write authority for the assistant or workers.
