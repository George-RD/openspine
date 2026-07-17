# Day-2 Operations Contract

This document describes the upgrade, first-run, downgrade, backup/restore, and failure posture required by AD-139 and AD-144.

## First-run and restart sequence

Run these steps in order; each boundary is retryable only as described:

1. **Load config and artifact key.** A missing or invalid config/key is a pre-store failure. Fix the inputs and retry; no database mutation is attempted.
2. **Open the artifact and secret stores.** Store-open errors stop startup before the kernel database is touched; retry after fixing the filesystem or key.
3. **Read and seed the Telegram token.** Read `telegram.bot_token` from the secret store, seeding it from configuration only when absent. A token read or seed error stops startup; retrying does not rerun migrations.
4. **Open the kernel store and apply migrations.** `Store::open` applies the ad-hoc baseline and versioned migrations before application data is used. Each versioned DDL plus `PRAGMA user_version` stamp is one transaction. A failed migration rolls back and is safe to retry.
5. **Validate the wall clock.** The current time is classified against the persisted high-water without writing it. A regression beyond 60 seconds stops startup; correct the host clock or restore a matching snapshot before retrying.
6. **Bootstrap the owner.** Owner principal creation is idempotent and transactional. A bootstrap error stops startup; retry does not lower the clock high-water because it has not been committed yet.
7. **Verify the audit chain.** Broken audit/provenance verification stops startup and requires repair or restore.
8. **Complete remaining setup and bind.** Load and validate the registry and overlays, initialize connectors and providers, construct application state, and bind the listener. Any failure stops startup before the clock high-water is persisted.
9. **Re-sample and commit the post-bind clock.** Read `Timestamp::now` only after bind succeeds. If it regressed more than 60 seconds against the pre-setup candidate, refuse startup without persisting; otherwise atomically commit the maximum of the pre-setup candidate and fresh sample so a tolerated regression cannot lower the high-water.

After the process is serving, the kernel timer driver records a durable wall-clock heartbeat each interval. Runtime observations are max-preserving, so a later restart cannot pass a clock-regression check using only an earlier boot timestamp.

## Telegram-first credential and connector sequence (AD-144)

The user-facing first-run sequence is deliberately Telegram-first:

1. **Bot token.** Export `OPENSPINE_TELEGRAM_BOT_TOKEN` and start the kernel. If the vault has no `telegram.bot_token`, startup seeds that value; a missing environment value fails with the exact config error `missing required environment variable OPENSPINE_TELEGRAM_BOT_TOKEN`. Startup wraps vault failures as `reading Telegram bot token from vault` or `seeding Telegram bot token`.
2. **Owner verification.** DM the bot from the account whose numeric ID is in `owner.telegram_user_id`. `telegram::verify_update` accepts only a matching sender in a private chat. Non-owner, missing-sender, non-text, missing callback-data, and group-chat updates are audited and ignored with these exact reason codes: `unknown_telegram_user`, `no_sender`, `non_text_update`, `callback_query_missing_data`, and `owner_message_outside_private_chat`. A valid DM is the only input that receives owner-control authority.
3. **Gmail OAuth.** Only after the owner-control lane is working, configure the optional `gmail` block and provide its client-secret and refresh-token environment variables. The headless kernel does not open a browser: complete Google's consent flow out of band, then provide the resulting refresh token. On the first Gmail operation the connector exchanges it at Google's token endpoint. Missing vault values fail with the exact messages `gmail token refresh failed: HTTP 0: gmail client secret is not configured` or `gmail token refresh failed: HTTP 0: gmail refresh token is not configured`; an HTTP failure is surfaced as `gmail token refresh failed: HTTP <status>: <body>`. If no `gmail` block exists, `/draft` replies exactly `Gmail isn't configured on this kernel yet`.

This ordering keeps bot-token acquisition and owner verification independent of Gmail OAuth. Gmail setup is optional for the Telegram-only lane and its OAuth failure must not be hidden as a successful first run.

## Schema migrations (`PRAGMA user_version`)

OpenSpine keeps an idempotent legacy lane for additive convergence and a transactional versioned lane. Future-version databases are rejected before DDL. The test-only downgrade helper applies `down` scripts in reverse order and updates the version stamp transactionally.

Migration v2 creates `boot_meta`, which stores `clock.high_water_ms`. **The v2 down migration drops `boot_meta` and therefore drops the clock high-water.** A subsequent upgrade must be treated as a new clock baseline: restore the host clock, take/verify the backup set, and run clock validation before serving. Operators must not downgrade and immediately serve on an unvalidated clock.

## Audit I/O failure handling

Audit writes fail closed. `SQLITE_FULL`, `SQLITE_READONLY`, and other database write errors propagate, return HTTP 500 for the action, and prevent connector side effects. The valid `TelegramReplyPayload { text }` path is tested so a zero connector request proves the audit failure happened before the effect.

## Consistent backup and restore

A snapshot is one stopped-process set, not independent files: `data_dir/kernel.db`, `data_dir/artifacts/`, `data_dir/credentials/`, and `data_dir/artifacts.d/`, together with the exact external `OPENSPINE_ARTIFACT_KEY`. Back up and restore all of them atomically as one set; the database, encrypted blobs, credential vault, overlays, and key must refer to the same point in time.

Restore procedure:

1. Stop the kernel.
2. Replace the entire data directory with the snapshot set.
3. Restore the exact artifact key securely and set permissions.
4. Synchronize and validate the host clock against the restored high-water (or explicitly accept a fresh high-water after v2-down).
5. Start the kernel and verify migration, owner bootstrap, audit-chain, and registry checks in logs.

```bash
chmod -R 700 data_dir/credentials data_dir/artifacts data_dir/artifacts.d
chmod 600 data_dir/kernel.db
```

## Failure messages and boundaries

- `unsupported database schema version X (latest supported is Y)`: use a newer binary or restore a matching snapshot; the file is not mutated.
- `wall clock regressed at boot ...`: synchronize the host clock or restore the matching database/blob/credential/key set; do not bypass the check.
- `audit_log hash chain is broken ...`: restore a trusted complete snapshot; do not serve on a damaged chain.
- Startup bind/setup failure: retry after correction; the candidate clock timestamp was validated but is not persisted until startup is otherwise ready.
