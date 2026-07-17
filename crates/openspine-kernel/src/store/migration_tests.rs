use super::Store;
use rusqlite::Connection;
use tempfile::tempdir;

#[test]
fn legacy_digest_summary_is_sanitized_idempotently() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("kernel.db");
    let secret = "LEGACY_DIGEST_SECRET_MUST_NOT_SURVIVE";
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE digest_items (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                id TEXT NOT NULL UNIQUE,
                ts TEXT NOT NULL,
                class TEXT NOT NULL,
                summary TEXT NOT NULL,
                resolved INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO digest_items (id, ts, class, summary, resolved) VALUES (?1, ?2, ?3, ?4, 0)",
            rusqlite::params![
                "01J00000000000000000000000",
                "2026-01-01T00:00:00Z",
                "connector",
                secret,
            ],
        )
        .unwrap();
    }
    let store = Store::open(&path).unwrap();
    let first = store.owner_digest_items().unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(
        first[0].summary,
        "[connector] legacy failure detail unavailable"
    );
    assert!(first[0].text_ref.is_none());
    assert!(!first[0].summary.contains(secret));
    drop(store);

    let reopened = Store::open(&path).unwrap();
    let second = reopened.owner_digest_items().unwrap();
    assert_eq!(second[0].summary, first[0].summary);
    assert!(!second[0].summary.contains(secret));
}
