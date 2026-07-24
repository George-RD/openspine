# Day-2 Operations Contract

This document describes the upgrade, first-run, downgrade, backup/restore, and failure posture required by AD-139, AD-144, and AD-150.

## First-run and restart sequence

Run these steps in order; each boundary is retryable only as described:

1. **Load config and artifact key.** A missing or invalid config/key is a pre-store failure. Fix the inputs and retry; no database mutation is attempted.
2. **Resolve data root and acquire lifetime lock.** Resolve a relative `data_dir` against the process working directory (`data` and `./data` are equivalent only within that same directory); reject parent traversal, symlink aliases, and non-directory roots. Canonicalize the parent and derive the expected absolute data-root path without creating a missing root. The adjacent control directory is `<canonical-parent>/.openspine-control-<sha256(canonical-absolute-data-root-path-bytes)>`. Open its `lifetime.lock` without following symlinks, acquire the exclusive lock, and retain the guard through shutdown. Only then may first boot create the data root. Launching the same relative config from another working directory intentionally selects another root; production launchers should therefore use a fixed working directory or absolute `data_dir`.
3. **Process signed pending operations.** Inspect pending export, restore, or rollback operations under the lifetime lock and before stores open. Execute the pre-open phase of any staged operation — copy/hash for export, verify/copy/install for restore, or rollback reinstallation — before proceeding to store initialization.
4. **Open the artifact and secret stores.** Store-open errors stop startup before the kernel database is touched; retry after fixing the filesystem or key.
5. **Read and seed the Telegram token.** Read `telegram.bot_token` from the secret store, seeding it from configuration only when absent. A token read or seed error stops startup; retrying does not rerun migrations.
6. **Open the kernel store and apply migrations.** `Store::open` applies the ad-hoc baseline and versioned migrations before application data is used. Each versioned DDL plus `PRAGMA user_version` stamp is one transaction. A failed migration rolls back and is safe to retry.
7. **Validate the wall clock.** The current time is classified against the persisted high-water without writing it. A regression beyond 60 seconds stops startup; correct the host clock or restore a matching snapshot before retrying.
8. **Verify audit and reconcile terminal erasures.** Before audit verification, pre-existing database erasure markers replay only into key tombstones to close the DB-commit-before-key-delete crash window. A broken audit/provenance chain then stops startup before owner bootstrap or any external-ledger database/audit reconciliation. After verification, reconcile the authenticated external terminal-erasure ledger into the generation before serving.
9. **Bootstrap the owner.** Owner principal creation is idempotent and transactional. A bootstrap error stops startup; retry does not lower the clock high-water because it has not been committed yet.
10. **Complete remaining setup and bind.** Load and validate the registry and overlays, initialize connectors and providers, construct application state, and bind the listener. Any failure stops startup before the clock high-water is persisted.
11. **Re-sample and commit the post-bind clock.** Read `Timestamp::now` only after bind succeeds. If it regressed more than 60 seconds against the pre-setup candidate, refuse startup without persisting; otherwise atomically commit the maximum of the pre-setup candidate and fresh sample so a tolerated regression cannot lower the high-water.

After the process is serving, the kernel timer driver records a durable wall-clock heartbeat each interval. Runtime observations are max-preserving, so a later restart cannot pass a clock-regression check using only an earlier boot timestamp.

## Telegram-first credential and connector sequence (AD-144)

The user-facing first-run sequence is deliberately Telegram-first:

1. **Bot token.** Export `OPENSPINE_TELEGRAM_BOT_TOKEN` and start the kernel. If the vault has no `telegram.bot_token`, startup seeds that value; a missing environment value fails with the exact config error `missing required environment variable OPENSPINE_TELEGRAM_BOT_TOKEN`. Startup wraps vault failures as `reading Telegram bot token from vault` or `seeding Telegram bot token`.
2. **Owner verification.** DM the bot from the account whose numeric ID is in `owner.telegram_user_id`. `telegram::verify_update` accepts only a matching sender in a private chat. Non-owner, missing-sender, non-text, missing callback-data, and group-chat updates are audited and ignored with these exact reason codes: `unknown_telegram_user`, `no_sender`, `non_text_update`, `callback_query_missing_data`, and `owner_message_outside_private_chat`. A valid DM is the only input that receives owner-control authority.
3. **Gmail OAuth.** Only after the owner-control lane is working, configure the optional `gmail` block and provide its client-secret and refresh-token environment variables. The headless kernel does not open a browser: complete Google's consent flow out of band, then provide the resulting refresh token. On the first Gmail operation the connector exchanges it at Google's token endpoint. Missing vault values fail with the exact messages `gmail token refresh failed: HTTP 0: gmail client secret is not configured` or `gmail token refresh failed: HTTP 0: gmail refresh token is not configured`; an HTTP failure is surfaced as `gmail token refresh failed: HTTP <status>: <body>`. If no `gmail` block exists, `/draft` replies exactly `Gmail isn't configured on this kern...`

This ordering keeps bot-token acquisition and owner verification independent of Gmail OAuth. Gmail setup is optional for the Telegram-only lane and its OAuth failure must not be hidden as a successful first run.

## Schema migrations (`PRAGMA user_version`)

OpenSpine keeps an idempotent legacy lane for additive convergence and a transactional versioned lane. Future-version databases are rejected before DDL. The test-only downgrade helper applies `down` scripts in reverse order and updates the version stamp transactionally.

Migration v2 creates `boot_meta`, which stores `clock.high_water_ms`. **The v2 down migration drops `boot_meta` and therefore drops the clock high-water.** A subsequent upgrade must be treated as a new clock baseline: restore the host clock, take/verify the backup set, and run clock validation before serving. Operators must not downgrade and immediately serve on an unvalidated clock.

## Audit I/O failure handling

Audit writes fail closed. `SQLITE_FULL`, `SQLITE_READONLY`, and other database write errors propagate, return HTTP 500 for the action, and prevent connector side effects. The valid `TelegramReplyPayload { text }` path is tested so a zero connector request proves the audit failure happened before the effect.

## Consistent export and restore (AD-150)

Export and restore are non-delegable root-owner actions that operate on one stopped-process authenticated directory bundle. The kernel never copies or replaces open storage; every operation requires a controlled restart.

### Canonical storage identity and lifetime lock

Startup resolves relative `data_dir` values against the process working directory, so `data` and `./data` share one identity only when launched from the same directory; an absolute path is safer for service managers. Parent traversal and symlink aliases are rejected. From the canonical parent and expected absolute data-root path, the kernel derives `<canonical-parent>/.openspine-control-<sha256(canonical-absolute-data-root-path-bytes)>`, opens its `lifetime.lock` without following symlinks, and retains the exclusive guard through shutdown. A missing data root is created only after the lock is held. Pending operations are inspected only after locking and before stores open.

The control directory owns `snapshots/`, pending-operation state, terminal-erasure state, and request-id staging/rollback paths. It is never replaced by restore.

### Gated request and controlled restart

`openspine.overlay.export` and `openspine.overlay.restore` are registered only in verified owner-control composition and catalogued non-delegable. Worker mint/commission rejects them. Handlers require a canonical sealed root grant (`parent_grant_id == None`, `root_grant_id == id`, exactly one parentless root chain step and no delegation hop), configured owner principal, and exact effective authority.

Payload is exactly `{ "bundle_name": <1..128 ASCII [A-Za-z0-9_-], not dot-prefixed> }`. The kernel derives `control/snapshots/<bundle_name>`; actions cannot name another path. Export requires that entry absent. Restore requires a pre-staged bundle directory entry that is neither a symlink nor special file. The signed request stores the concrete bundle name plus operation, request id, action id, owner principal id, grant id, and timestamp; audit uses only a digest of the derived path. Pre-open execution revalidates the protected snapshot root, entry type, name, and existence rules under the lifetime lock.

Requests are master-key-HMAC authenticated and written temp -> file fsync -> rename -> control-directory fsync. Only one may be pending. Handlers return `restart_required` and never copy/replace open storage.

### Export procedure

1. **Request.** The verified root owner invokes `openspine.overlay.export` with an allowed root grant and a valid unused bundle name. The action is gated and audited; one signed operation is staged. The response requires restart.
2. **Stop the kernel.**
3. **Restart.** Under the lifetime lock and before stores open, the pending export is detected. The kernel copies/hashes the complete configured `data_dir` at-rest representation (`kernel.db`, `artifacts/`, `keys/`, `credentials/`, and `artifacts.d/`) into a unique temporary directory under the protected snapshot root.
4. **Authenticate the typed tree.** The bundle manifest authenticates format/request metadata, a signed terminal-erasure-ledger snapshot, and a sorted bijection with every typed entry under `data/`: each directory exactly once and each regular file exactly once with byte length/SHA-256. Only normalized declared directories and files are permitted. Empty/unlisted directories, duplicate/missing/extra paths, wrong types, symlinks, special files, and non-normal paths fail. Directories are `0700`, files `0600`, and tombstones are regular files.
5. **Publish atomically.** The kernel fsyncs files/directories/manifest bottom-up and publishes with one rename plus parent fsync. A pre-existing final bundle is not replaced. The external `OPENSPINE_ARTIFACT_KEY` is not copied into the bundle; the raw at-rest representation remains sensitive.
6. **Preserve external prerequisites.** The operator preserves the exact `OPENSPINE_ARTIFACT_KEY` and the latest signed terminal-erasure ledger separately. Both are required for portable restore.
7. **Startup completes normally.** After publication the kernel continues through migration, clock, audit verification, terminal-erasure reconciliation, owner bootstrap, overlay, registry, provider/connector, listener bind, and post-bind clock checks. Export completion is audited before the marker is removed.

### Restore procedure

1. **Stage the bundle.** Place the authenticated directory bundle under `control/snapshots/<bundle_name>/` (the protected snapshot root). The bundle is a raw directory tree with restrictive modes, not an archive.
2. **Request.** The verified root owner invokes `openspine.overlay.restore` with an allowed root grant and the staged bundle name. The action is gated and audited; one signed operation is staged. The response requires restart.
3. **Stop the kernel.**
4. **Restart.** Under the lifetime lock and before stores open, the pending restore is detected. The kernel verifies manifest schema/HMAC/typed set, then opens each source without following symlinks and copies into same-filesystem staging while hashing/counting. It verifies the completed staged typed tree again. After this point it never reads source bundle bytes.
5. **Merge erasure continuity.** The kernel requires the bundle baseline and destination ledger to share the same HMAC-bound random continuity id, then merges their monotonic sequence/set. A fresh unrelated ledger fails even at sequence zero. Same-host restore uses its live lineage. Portable restore requires the separately preserved latest source ledger. Pre-open restore applies every merged terminal id to staging with regular tombstones and key deletion, then revalidates the exact authorized typed-tree delta.
6. **Install through crash-recoverable stages.** Replacement uses request-id paths inside the canonical control/staging area and canonical data-root parent: `restore-new` candidate, `restore-old` retained generation, `restore-rejected` candidate after rollback. Signed stages `requested -> staged -> installed -> finalizing` and parent fsync make every rename combination idempotently recoverable. The old generation remains until full startup finalization.
7. **Full startup contract.** The installed generation passes all normal checks in order: migration, clock, audit verification, terminal-erasure reconciliation, owner bootstrap, overlay compatibility/admission, registry/persona/model/provider/connector checks, listener bind, and post-bind clock commit. The restored chain receives idempotent digest-safe `overlay.restore_requested` authorization evidence followed by `overlay.restore_completed` before old data/control state is removed.
8. **Finalization.** Only after the full contract passes are old data and the signed marker removed. Audit/cleanup failure remains retryable without recopy/reinstall.

### Portable restore prerequisites

A bundle transferred to a fresh host requires both external prerequisites:

- **`OPENSPINE_ARTIFACT_KEY`** — the exact artifact master key used when the bundle was exported.
- **Latest signed terminal-erasure ledger** — the signed ledger at least as new as the bundle's embedded baseline.

Without both, restore fails before moving active data. On a fresh host, stage these files before its first boot so the kernel adopts the preserved lineage instead of minting an unrelated one:

1. Resolve the destination `data_dir` exactly as the service will: relative paths are anchored to that service's fixed working directory. Derive the adjacent control root as `<canonical-parent>/.openspine-control-<sha256(canonical-absolute-data-root-path-bytes)>`.
2. Create that control root and its `snapshots/` child with mode `0700`. Install the raw bundle at `snapshots/<bundle_name>/`, preserving its modes.
3. Copy the preserved raw signed ledger to `<control-root>/terminal-erasure-ledger.json` with mode `0600`; do not wrap or rewrite its JSON.
4. Set `OPENSPINE_ARTIFACT_KEY` to the preserved key and start the kernel. Under the lifetime lock, first boot authenticates the pre-placed ledger, creates the matching generation marker, then bootstraps the destination owner.
5. The verified destination root owner requests `openspine.overlay.restore`; stop and restart as directed. The source bundle's authenticated owner/grant/request remain replay evidence but need not equal the destination's newly generated principal id.

### Offline rollback: `--rollback-pending-restore`

If an installed restore fails startup (provider validation, listener bind, or post-bind clock commit), the operator invokes authenticated pathless rollback:

```bash
openspine --rollback-pending-restore
```

This acquires the same lifetime lock, verifies the one signed pending restore, and accepts no directory argument. It durably marks `rollback-requested`, moves rejected active data aside, reinstalls retained old data with parent fsync, then runs normal startup. After the full contract passes, the old chain receives `overlay.restore_rolled_back`; rejected data and marker are removed. Crash recovery covers each rollback rename boundary. Automatic rollback is rejected because it could hide tampering or incompatibility.

### Failure messages and boundaries

- `unsupported database schema version X (latest supported is Y)`: use a newer binary or restore a matching snapshot; the file is not mutated.
- `wall clock regressed at boot ...`: synchronize the host clock or restore the matching database/blob/credential/key set; do not bypass the check.
- `audit_log hash chain is broken ...`: restore a trusted complete snapshot; do not serve on a damaged chain.
- Startup bind/setup failure: retry after correction; the candidate clock timestamp was validated but is not persisted until startup is otherwise ready.
- `export/restore requires a canonical sealed root grant with no delegation hop`: the action is non-delegable; only the configured owner principal may invoke it directly.
- `bundle name must be 1..128 ASCII alphanumeric, underscore, or hyphen, not dot-prefixed`: the payload was rejected before any filesystem location was touched.
- `pending operation already exists`: only one export or restore may be pending at a time; complete or roll back the current operation first.
- `bundle manifest HMAC does not verify`: the bundle was tampered with or the wrong artifact key is in use; do not proceed.
- `terminal-erasure ledger continuity check failed`: the destination ledger has a different continuity id or precedes the bundle baseline; install the latest preserved source ledger or use the same-host lineage.
- `portable restore requires a separately preserved latest signed erasure ledger`: the fresh host has no established ledger; supply the preserved ledger before requesting restore.
- `restore validation failed: source changed during copy`: copy-time hashing or staged exact-tree validation failed; the source bundle was modified after initial verification.
- `rollback requires a signed pending restore`: no installed generation is pending finalization; `--rollback-pending-restore` is a no-op.
- `rollback completed`: the prior generation is active and passed startup; rejected data has been removed.

### Same-host vs. fresh-host continuity

| Aspect | Same-host restore | Fresh-host (portable) restore |
|---|---|---|
| Erasure ledger | Live control-directory ledger used automatically | Requires separately preserved latest signed ledger |
| Artifact key | Already configured as `OPENSPINE_ARTIFACT_KEY` | Must be set to the preserved key |
| Bundle transport | Already under `control/snapshots/` | Must be copied into `control/snapshots/` |
| Ledger continuity | Same HMAC-bound continuity id used automatically | Preserved source ledger required; unrelated fresh id fails |
| Rollback | Old generation retained through finalization | Old generation is the fresh host's pre-restore state |

### Bundle permissions

- every bundle/data directory: `0700`
- every manifest/data regular file: `0600`

The kernel enforces these modes during export; the operator must preserve them during transport. The bundle is a raw directory tree — never archive, compress, or re-hydrate it in a way that loses mode bits or introduces symlinks.
