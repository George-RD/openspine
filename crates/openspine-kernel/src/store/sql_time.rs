//! Canonical, order-stable RFC 3339 rendering for `TEXT` timestamp columns.
//!
//! Several tables store an instant as an RFC 3339 `TEXT` column and then
//! compare it with an inequality in SQL — `next_attempt_at <= ?1`,
//! `expires_at > ?5`, `created_at < ?1`, `occurred_at > ?2`. SQLite compares
//! `TEXT` with plain byte ordering, so those queries are only correct if the
//! rendering is a *monotone* encoding: `a <= b` as bytes exactly when
//! `a <= b` as instants.
//!
//! `Timestamp`'s `Display` is NOT such an encoding. jiff prints the smallest
//! faithful fraction: trailing zeros are trimmed, and a whole-second instant
//! gets no fractional part at all. That makes the byte order disagree with
//! time order inside a single second, because `'Z'` (0x5A) sorts after `'.'`
//! (0x2E):
//!
//! ```text
//!   "2026-08-07T04:22:57Z"  >  "2026-08-07T04:22:57.000123456Z"
//! ```
//!
//! The left instant is a tenth of a millisecond *earlier*, yet compares
//! greater. A dead letter enqueued exactly on a second boundary was therefore
//! not claimable until the next second, and a selection token expiring on a
//! second boundary read as un-expired for up to a second — a fail-open window
//! on `expires_at > now`.
//!
//! Rendering with a fixed nine-digit fraction removes the variability: every
//! timestamp in the supported range produces the same-width string, so byte
//! order and time order coincide. Use [`sql_timestamp`] for every write to,
//! and every comparison against, a `TEXT` timestamp column that participates
//! in an inequality or an `ORDER BY`. Columns compared only for equality are
//! unaffected, and columns stored as epoch-nanosecond `INTEGER`s already sort
//! correctly.

use jiff::Timestamp;

/// Render `timestamp` as fixed-width RFC 3339 with a nine-digit fraction, so
/// SQLite's byte comparison of the column agrees with instant ordering.
///
/// Round-trips through `str::parse::<Timestamp>()` unchanged.
pub(crate) fn sql_timestamp(timestamp: Timestamp) -> String {
    format!("{timestamp:.9}")
}

#[cfg(test)]
mod tests {
    use super::sql_timestamp;
    use jiff::Timestamp;

    fn at(seconds: i64, nanos: i32) -> Timestamp {
        Timestamp::new(seconds, nanos).expect("representable instant")
    }

    #[test]
    fn whole_second_sorts_before_a_later_instant_in_the_same_second() {
        // The exact defect that made `next_attempt_at <= ?1` miss a due row
        // and `expires_at > ?5` keep an expired selection token alive.
        let boundary = at(1_775_000_000, 0);
        let just_after = at(1_775_000_000, 123_456);
        assert!(boundary < just_after);
        assert!(
            sql_timestamp(boundary) < sql_timestamp(just_after),
            "byte order must follow instant order across the second boundary"
        );
        assert!(
            boundary.to_string() > just_after.to_string(),
            "Display is not order-stable here; this is why sql_timestamp exists"
        );
    }

    #[test]
    fn trimmed_trailing_zeros_do_not_invert_the_order() {
        // `Display` renders 500_000_000ns as ".5", a prefix of the later
        // ".500000001", so the shorter string's terminator outranks a digit.
        let earlier = at(1_775_000_000, 500_000_000);
        let later = at(1_775_000_000, 500_000_001);
        assert!(earlier < later);
        assert!(sql_timestamp(earlier) < sql_timestamp(later));
    }

    #[test]
    fn every_rendering_has_the_same_width() {
        let widths = [
            sql_timestamp(at(1_775_000_000, 0)).len(),
            sql_timestamp(at(1_775_000_000, 1)).len(),
            sql_timestamp(at(1_775_000_000, 999_999_999)).len(),
            sql_timestamp(at(1_000_000_000, 500_000_000)).len(),
        ];
        assert!(
            widths.iter().all(|width| *width == widths[0]),
            "fixed width is what makes byte order total: {widths:?}"
        );
    }

    #[test]
    fn round_trips_through_parse() {
        let original = at(1_775_000_000, 123_456_789);
        let parsed: Timestamp = sql_timestamp(original).parse().expect("round trip");
        assert_eq!(parsed, original);
        let whole = at(1_775_000_000, 0);
        let parsed_whole: Timestamp = sql_timestamp(whole).parse().expect("round trip");
        assert_eq!(parsed_whole, whole);
    }
}
