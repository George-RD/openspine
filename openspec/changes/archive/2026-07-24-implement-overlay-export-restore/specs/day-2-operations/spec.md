## MODIFIED Requirements

### Requirement: One-set snapshot and restore

A day-two backup MUST be requested through the non-delegable root-owner `openspine.overlay.export` action using a bounded name inside the kernel-controlled snapshot root and completed under the canonical data-directory lifetime lock on restart before stores open. It MUST atomically publish one authenticated directory bundle containing the complete configured `data_dir` at-rest representation (`kernel.db`, `artifacts/`, `keys/`, `credentials/`, and `artifacts.d/`), an exact typed-tree manifest, restrictive `0700`/`0600` modes, and a signed terminal-erasure-ledger baseline. The external `OPENSPINE_ARTIFACT_KEY` and latest signed terminal-erasure ledger MUST be preserved separately for portable recovery.

Restore MUST be requested through the equivalent root-owner action after a bundle is staged in the protected snapshot root. It MUST copy-hash/validate the exact typed tree into same-filesystem staging, merge non-regressing erasure continuity, install through crash-recoverable new/old stages, and retain the previous generation until the complete normal startup contract passes. Migration, owner bootstrap, audit-chain verification, clock and erasure reconciliation, overlay compatibility/admission, provider/connector checks, listener bind, and post-bind clock commit MUST pass before serving or cleanup. A failed installed restore MUST support authenticated pathless offline rollback.

#### Scenario: One authenticated snapshot restores coherently
- **WHEN** the verified root owner gates a named export, restarts to publish it, separately preserves the exact artifact master key and latest signed erasure ledger, stages/gates a restore, and restarts
- **THEN** one authenticated point-in-time data generation is installed with restrictive permissions, terminal erasures remain closed, all startup/compatibility checks pass before serving, and old data is removed only after auditable finalization

#### Scenario: Portable restore lacks external continuity
- **WHEN** a transferred bundle reaches a fresh host without the matching external master key or a signed terminal-erasure ledger at least as new as its embedded baseline
- **THEN** restore fails before moving active data

#### Scenario: Installed generation fails a late startup check
- **WHEN** a restored generation fails provider validation, listener bind, or post-bind clock commit
- **THEN** the signed pending restore and previous generation remain available and the documented authenticated offline rollback restores the prior generation before serving
