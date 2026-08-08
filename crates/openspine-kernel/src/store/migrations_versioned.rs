//! The ordered versioned-migration tables and their documented down paths.
//! Split from `migrations.rs` to keep both files under the 500-line gate; the
//! runner, the ad-hoc additive lane, and the stamping logic stay there.

use super::migrations::VersionedMigration;

/// Forward versioned migrations beyond [`BASELINE_USER_VERSION`].
/// Entry v2 adds the `boot_meta` table that backs boot clock-regression
/// detection ([`super::boot_clock`]); it is purely additive and reversible.
/// Entry v3 adds the `skills.schema_version` column (AD-040/AD-041): the
/// ad-hoc `ensure_schema` deliberately omits it, so a fresh or legacy
/// `skills` table converges here via an additive `ALTER TABLE ... ADD COLUMN`
/// (DEFAULT 1, the only supported version) without rewriting any row.
pub(super) const VERSIONED_MIGRATIONS: &[VersionedMigration] = &[
    VersionedMigration {
        version: 2,
        up: "CREATE TABLE IF NOT EXISTS boot_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    },
    VersionedMigration {
        version: 3,
        up: "ALTER TABLE skills ADD COLUMN schema_version INTEGER NOT NULL DEFAULT 1;",
    },
    // Entry v4 normalizes the `TEXT` timestamp columns that SQL compares with
    // an inequality or orders by. They were written with jiff's `Display`,
    // which trims trailing fractional zeros and drops the fraction entirely on
    // a whole second — so byte order disagreed with instant order inside a
    // second (`'Z'` 0x5A sorts after `'.'` 0x2E). See `store::sql_time`.
    // Rewrites each legacy value (length 20..29) to the fixed-width
    // nine-digit form new writes use; canonical rows (length 30) and NULLs are
    // left alone, so the migration is idempotent and never invents an instant.
    VersionedMigration {
        version: 4,
        up: TIMESTAMP_NORMALIZATION_SQL,
    },
    // Entry v5 marks the schema that carries the two standing-rule scope
    // binding digest columns (`reviewed_scope_digest`, `compatibility_digest`).
    // The columns themselves are added additively by the ad-hoc lane
    // (`add_column_if_missing`, idempotent across every open), so a legacy DB
    // converges without a destructive rewrite and the versioned entry is a
    // pure stamp — the same additive/versioned split AD-139 draws. Kept as a
    // versioned stamp so the down path can drop the columns for the documented
    // downgrade and future destructive work is ordered after it.
    VersionedMigration { version: 5, up: "" },
    // Entry v6 marks the schema that carries `dark_window_max_pending`, the
    // reviewed outstanding-exception allowance. Added additively by the ad-hoc
    // lane for the same reason v5 was; the versioned entry is a pure stamp
    // whose down path drops the column.
    VersionedMigration { version: 6, up: "" },
    // Entry v7 marks the schema that carries the eval-verdict epoch columns
    // (#133): the proposal, compatibility, reviewed-scope and evidence-set
    // digests plus the descriptor, implementation and policy versions a
    // verdict was computed under. Added additively by the ad-hoc lane for the
    // same reason v5 and v6 were; the versioned entry is a pure stamp whose
    // down path drops the columns. No backfill: a pre-#133 row records no
    // epochs and is therefore compared on no axis.
    VersionedMigration { version: 7, up: "" },
    // Entry v8 is the first *destructive* migration: it rebuilds
    // `standing_rules` to add a `CHECK(status IN ('active','paused',
    // 'needs_review','revoked'))` constraint, so the owner-controlled `paused`
    // status introduced by #129 is a typed, DB-enforced value rather than free
    // -form text. Every existing row is preserved verbatim.
    VersionedMigration {
        version: 8,
        up: super::migration_owner_review::PAUSED_STATUS_UP,
    },
    // Entry v9 completes the channel-neutral owner-surface cutover: the ad-hoc
    // lane adds `owner_surface_json` to `task_grants` and
    // `standing_rule_pending_actions`, and this destructive rebuild drops the
    // Telegram-shaped `bound_chat_id` column from both. After it, no generic
    // kernel seam — grant, pending action, notification, receipt — persists a
    // naked chat id.
    VersionedMigration {
        version: 9,
        up: super::migration_owner_review::OWNER_SURFACE_UP,
    },
];

/// Rewrite `col` from jiff's variable-width RFC 3339 to the fixed nine-digit
/// fraction. `substr(col, 1, 19)` is `YYYY-MM-DDTHH:MM:SS`; anything past
/// position 20 up to the trailing `Z` is the existing fraction, which is
/// right-padded with zeros and clipped to nine digits.
///
/// Precondition: a four-digit, non-negative year, which is what every writer
/// in this crate produces (`jiff::Timestamp` is UTC and the store holds only
/// runtime instants). An expanded-year rendering such as `-0001-01-01T…`
/// would shift the fixed offsets, so the guard below also bounds the length —
/// but a future column admitting expanded years MUST NOT reuse this macro.
macro_rules! normalize_timestamp_column {
    ($table:literal, $col:literal) => {
        concat!(
            "UPDATE ",
            $table,
            " SET ",
            $col,
            " = substr(",
            $col,
            ", 1, 19) || '.' || substr(",
            "CASE WHEN length(",
            $col,
            ") > 20 THEN substr(",
            $col,
            ", 21, length(",
            $col,
            ") - 21) ELSE '' END || '000000000', 1, 9) || 'Z' WHERE ",
            $col,
            " IS NOT NULL AND length(",
            $col,
            ") BETWEEN 20 AND 29;"
        )
    };
}

const TIMESTAMP_NORMALIZATION_SQL: &str = concat!(
    normalize_timestamp_column!("notify_dead_letters", "enqueued_at"),
    normalize_timestamp_column!("notify_dead_letters", "next_attempt_at"),
    normalize_timestamp_column!("notify_dead_letters", "claimed_until"),
    normalize_timestamp_column!("task_grants", "expires_at"),
    normalize_timestamp_column!("skill_context_selections", "expires_at"),
    normalize_timestamp_column!("connector_restart_ledger", "occurred_at"),
    normalize_timestamp_column!("worker_dispatch", "created_at"),
);

#[cfg(test)]
pub(super) fn timestamp_normalization_sql_for_test() -> &'static str {
    TIMESTAMP_NORMALIZATION_SQL
}

/// Inverse `down` SQL for each versioned migration (AD-139 downgrade path),
/// test-only — production never reverts. Kept in lockstep with
/// [`VERSIONED_MIGRATIONS`] by version.
#[cfg(test)]
pub(super) const VERSIONED_DOWNS: &[(i64, &str)] = &[
    (2, "DROP TABLE IF EXISTS boot_meta;"),
    (3, "ALTER TABLE skills DROP COLUMN schema_version;"),
    // v4 normalized values, not schema: the pre-v4 renderings parse to the
    // same instants, and older code reads the canonical form correctly, so
    // the documented downgrade only rolls the version stamp back.
    (4, ""),
    // v5 added the two nullable scope-binding digest columns; the downgrade
    // drops them. Rows are untouched (NULL columns on legacy unbounded rules).
    (
        5,
        "ALTER TABLE standing_rules DROP COLUMN reviewed_scope_digest;
         ALTER TABLE standing_rules DROP COLUMN compatibility_digest;",
    ),
    // v6 added the nullable exception-allowance column; the downgrade drops
    // it. Rows are untouched (a dropped column reads back as the safe default).
    (
        6,
        "ALTER TABLE standing_rules DROP COLUMN dark_window_max_pending;
         ALTER TABLE standing_rule_pending_actions DROP COLUMN reviewed_scope_digest;
         ALTER TABLE standing_rule_pending_actions DROP COLUMN compatibility_digest;",
    ),
    // v7 added the seven nullable eval-verdict epoch columns; the downgrade
    // drops them. Rows are untouched.
    (
        7,
        "ALTER TABLE eval_verdicts DROP COLUMN proposal_digest;
         ALTER TABLE eval_verdicts DROP COLUMN compatibility_digest;
         ALTER TABLE eval_verdicts DROP COLUMN reviewed_scope_digest;
         ALTER TABLE eval_verdicts DROP COLUMN evidence_set_digest;
         ALTER TABLE eval_verdicts DROP COLUMN descriptor_version;
         ALTER TABLE eval_verdicts DROP COLUMN implementation_version;
         ALTER TABLE eval_verdicts DROP COLUMN policy_version;",
    ),
    // v8 removes the paused-status CHECK while preserving every row.
    (8, super::migration_owner_review::PAUSED_STATUS_DOWN),
    // v9 restores the legacy `bound_chat_id` column on both tables; rows keep
    // their channel-neutral `owner_surface_json` alongside it.
    (9, super::migration_owner_review::OWNER_SURFACE_DOWN),
];
