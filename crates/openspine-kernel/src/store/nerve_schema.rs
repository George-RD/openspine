//! Nerve table DDL, split out of `store/nerve.rs` to keep that module under the
//! 500-line gate. Called once per connection at open (`Store::ensure_schema`).

use rusqlite::Connection;

pub(super) fn ensure_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS nerve_registrations (
            nerve_id TEXT PRIMARY KEY,
            advisee_id TEXT NOT NULL,
            declaration_json TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS nerve_advisee_limits (
            advisee_id TEXT PRIMARY KEY,
            scope_json TEXT NOT NULL,
            max_tier TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS nerve_interjection_budgets (
            nerve_id TEXT NOT NULL,
            window_kind TEXT NOT NULL,
            window_started_ns INTEGER NOT NULL,
            used INTEGER NOT NULL DEFAULT 0,
            max INTEGER NOT NULL,
            PRIMARY KEY (nerve_id, window_kind),
            FOREIGN KEY (nerve_id) REFERENCES nerve_registrations(nerve_id)
        );
        CREATE TABLE IF NOT EXISTS nerve_decay (
            nerve_id TEXT NOT NULL,
            class TEXT NOT NULL,
            ignored_count INTEGER NOT NULL DEFAULT 0,
            retired INTEGER NOT NULL DEFAULT 0,
            engaged_count INTEGER NOT NULL DEFAULT 0,
            annoyed_count INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (nerve_id, class),
            FOREIGN KEY (nerve_id) REFERENCES nerve_registrations(nerve_id)
        );
        CREATE TABLE IF NOT EXISTS nerve_issuances (
            interjection_id TEXT PRIMARY KEY,
            nerve_id TEXT NOT NULL,
            advisee_id TEXT NOT NULL,
            nerve_type TEXT NOT NULL,
            class_digest TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS nerve_reactions (
            interjection_id TEXT PRIMARY KEY,
            reaction TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS nerve_interjection_deliveries (
            interjection_id TEXT PRIMARY KEY,
            class_digest TEXT NOT NULL,
            gate_visible INTEGER NOT NULL
        );",
    )
}
