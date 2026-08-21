//! Dead-letter owner-surface migration tests (#217, versioned entry v10).
//! Kept in a sibling file so `migration_dead_letter_surface.rs` stays under the
//! 500-line gate and to match the `migration_owner_surface_tests` convention.

use std::collections::BTreeSet;

use openspine_schemas::owner_surface::OwnerSurfaceKind;
use rusqlite::{params, Connection};
use tempfile::tempdir;

use super::{migrations, Store, StoreError};

fn column_names(conn: &Connection, table: &str) -> BTreeSet<String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap();
    let names = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    names
}

/// v10 backfills a legacy Telegram `chat_id` into the channel-neutral
/// `owner_surface_json`, promoting it to a `verified_telegram` surface bound to
/// the single owner principal, then drops the integer column. The up/down/up
/// path round-trips: the v10 downgrade restores `chat_id`, and the re-open
/// applies v10 again, dropping it.
#[test]
fn v10_backfills_chat_id_into_owner_surface_and_round_trips() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("kernel.db");

    // Fresh open reaches v10; bind the single owner so the backfill resolves.
    let store = Store::open(&path).unwrap();
    let owner = store.bootstrap_owner_principal(42, "George").unwrap();

    // Down to v9 restores the legacy `chat_id` column; stage a pre-v10 row.
    {
        let mut conn = store.conn.lock();
        migrations::revert_versioned_migrations_for_test(&mut conn, 9).unwrap();
        assert!(
            column_names(&conn, "notify_dead_letters").contains("chat_id"),
            "the v10 downgrade restores the legacy chat_id column"
        );
        conn.execute(
            "INSERT INTO notify_dead_letters \
             (id, enqueued_at, chat_id, owner_surface_json, text_ref, task_grant_id, next_attempt_at) \
             VALUES (?1, '2026-01-01T00:00:00Z', 424242, NULL, 'ref', ?2, '2099-01-01T00:00:00Z')",
            params![
                ulid::Ulid::new().to_string(),
                ulid::Ulid::new().to_string()
            ],
        )
        .unwrap();
    }
    drop(store);

    // Re-open applies v10 up: the legacy chat id is promoted to a surface bound
    // to the owner principal, and the Telegram-shaped column is dropped.
    let store = Store::open(&path).unwrap();
    {
        let conn = store.conn.lock();
        assert!(
            !column_names(&conn, "notify_dead_letters").contains("chat_id"),
            "v10 drops the Telegram-shaped chat_id column"
        );
    }
    let dl = store.pending_dead_letters().unwrap();
    assert_eq!(dl.len(), 1, "the legacy row survives the migration");
    let surface = &dl[0].owner_surface;
    assert_eq!(surface.kind(), OwnerSurfaceKind::TelegramPrivate);
    assert_eq!(
        surface.surface_id(),
        Some("424242"),
        "the legacy chat id is preserved as the connector surface id"
    );
    assert_eq!(
        surface.principal_id(),
        owner.id,
        "the promoted surface is bound to the single owner principal"
    );
}

/// A dead-letter row whose `owner_surface_json` is NULL (an unresolvable owner
/// binding the migration could not promote) is refused at read time rather than
/// delivered — the `hydrate_task_grant` fail-closed precedent.
#[test]
fn null_owner_surface_fails_closed_on_read() {
    let store = Store::open_in_memory().unwrap();
    // A v10 file has no chat_id column; stage a row with a NULL surface.
    store
        .conn
        .lock()
        .execute(
            "INSERT INTO notify_dead_letters \
             (id, enqueued_at, owner_surface_json, text_ref, task_grant_id, next_attempt_at, state) \
             VALUES (?1, '2026-01-01T00:00:00Z', NULL, 'ref', ?2, '2000-01-01T00:00:00Z', 'pending')",
            params![
                ulid::Ulid::new().to_string(),
                ulid::Ulid::new().to_string()
            ],
        )
        .unwrap();

    assert!(
        matches!(
            store.pending_dead_letters(),
            Err(StoreError::BadOwnerSurface(_))
        ),
        "a NULL owner surface is refused, not read as an empty binding"
    );
    assert!(
        matches!(
            store.claim_due_dead_letter(jiff::Timestamp::now()),
            Err(StoreError::BadOwnerSurface(_))
        ),
        "the claim path fails closed on an unresolvable surface"
    );
}
