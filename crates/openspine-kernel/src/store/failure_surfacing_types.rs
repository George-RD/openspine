use jiff::Timestamp;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::Digest;
use rusqlite::Transaction;
use ulid::Ulid;

use super::StoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigestItem {
    pub id: Ulid,
    pub ts: Timestamp,
    pub class: String,
    pub summary: String,
    /// Digest of the encrypted artifact containing sensitive detail.
    /// `None` is an explicitly legacy row with no protected detail.
    pub text_ref: Option<String>,
    pub resolved: bool,
}

/// Max characters retained in `digest_items.summary` (the bounded,
/// non-sensitive description). Anything longer is encrypted as a
/// `text_ref` artifact and only the bounded prefix stays in SQLite
/// (D-012: the store layer must not become a plaintext privacy surface).
pub(crate) const MAX_DIGEST_SUMMARY_CHARS: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum DeadLetterState {
    Pending,
    InProgress,
    Resolved,
}

impl DeadLetterState {
    #[cfg(test)]
    pub fn parse(s: &str) -> Result<Self, StoreError> {
        match s {
            "pending" => Ok(Self::Pending),
            "in_progress" => Ok(Self::InProgress),
            "resolved" => Ok(Self::Resolved),
            other => Err(StoreError::FailureRouting(format!(
                "unparseable dead-letter state {other}"
            ))),
        }
    }
}

/// Semantic kind stored on `notify_dead_letters.semantic_kind` for a
/// `/digest <ULID>` detail delivery. Generic owner notifications leave the
/// column NULL so the retry worker takes the generic `owner.notified` path.
pub const DETAIL_SEMANTIC_KIND: &str = "digest_detail";

/// Availability outcome stored on `notify_dead_letters.availability_outcome`
/// for a detail delivery whose protected detail was resolvable and viewed.
pub const OUTCOME_AVAILABLE: &str = "available";

/// Build the stored availability-outcome value for an unavailable detail
/// (`"unavailable:<reason>"`), preserving the reason losslessly.
pub(crate) fn unavailable_outcome(reason: &str) -> String {
    format!("unavailable:{reason}")
}

/// Split a stored availability outcome into `(viewable, reason)`. Returns
/// `None` only for a generic (NULL) row.
pub(crate) fn parse_availability_outcome(outcome: Option<&str>) -> Option<(bool, Option<String>)> {
    match outcome {
        None => None,
        Some(OUTCOME_AVAILABLE) => Some((true, None)),
        Some(other) if other.starts_with("unavailable:") => Some((
            false,
            other.strip_prefix("unavailable:").map(str::to_string),
        )),
        // Fail closed: an unknown outcome is treated as unavailable rather
        // than silently emitting a `failure.digest_detail_viewed` receipt.
        Some(other) => Some((false, Some(format!("unparseable outcome: {other}")))),
    }
}

/// Map optional detail metadata to the five nullable `notify_dead_letters`
/// columns. `None` (generic notification) yields all-NULL columns.
#[allow(clippy::type_complexity)]
pub(crate) fn detail_insert_columns(
    detail: Option<&DetailReceipt>,
) -> (
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<i64>,
    Option<String>,
) {
    match detail {
        Some(d) => (
            Some(DETAIL_SEMANTIC_KIND.to_string()),
            d.detail_ref.clone(),
            Some(d.page_index as i64),
            Some(d.page_count as i64),
            Some(d.availability_outcome()),
        ),
        None => (None, None, None, None, None),
    }
}

/// Semantic metadata for a `/digest <ULID>` detail delivery, carried through
/// the dead-letter queue so the retry worker can append the contract-specific
/// `failure.digest_detail_viewed`/`unavailable` receipt — with full
/// detail-ref and page metadata — on retry success. Generic notifications
/// have `None` for all of these.
#[derive(Clone)]
pub struct DetailReceipt {
    /// Artifact ref of the failure's protected detail (the `refs` attached to
    /// the audit receipt). `None` only for a legacy row with no `text_ref`.
    pub detail_ref: Option<String>,
    pub page_index: usize,
    pub page_count: usize,
    /// `None` ⇒ the detail was viewable; `Some(reason)` ⇒ unavailable.
    pub unavailable_reason: Option<String>,
}

impl DetailReceipt {
    /// Stored value for `availability_outcome` (`"available"` or
    /// `"unavailable:<reason>"`).
    pub(crate) fn availability_outcome(&self) -> String {
        match &self.unavailable_reason {
            None => OUTCOME_AVAILABLE.to_string(),
            Some(reason) => unavailable_outcome(reason),
        }
    }

    /// Lossless audit `reason` for the receipt: both viewed and unavailable
    /// receipts carry `page=N/M` so the pagination context survives.
    pub(crate) fn receipt_reason(&self) -> String {
        match &self.unavailable_reason {
            None => format!("page={}/{}", self.page_index, self.page_count),
            Some(reason) => {
                format!("{reason}; page={}/{}", self.page_index, self.page_count)
            }
        }
    }

    /// Receipt kind for this outcome.
    pub(crate) fn receipt_kind(&self) -> &'static str {
        if self.unavailable_reason.is_none() {
            "failure.digest_detail_viewed"
        } else {
            "failure.digest_detail_unavailable"
        }
    }
    /// Append the contract-specific receipt inside an existing transaction
    /// (the DLQ-completion transaction), so the receipt and the row state
    /// commit atomically and the `claim_token` fence gates a duplicate write.
    pub(crate) fn append_in_tx(&self, tx: &Transaction) -> Result<(), StoreError> {
        let detail_refs = self
            .detail_ref
            .as_deref()
            .and_then(|r| Digest::parse(r).ok())
            .map(|digest| {
                vec![ArtifactRef {
                    digest,
                    schema_version: 1,
                }]
            });
        let refs_slice = detail_refs.as_ref().map_or(&[][..], |r| &r[..]);
        crate::store::Store::append_audit_conn(
            tx,
            self.receipt_kind(),
            None,
            None,
            Some(&self.receipt_reason()),
            None,
            &[],
            refs_slice,
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct NotifyDeadLetter {
    pub id: Ulid,
    pub enqueued_at: Timestamp,
    pub chat_id: i64,
    pub text_ref: String,
    pub task_grant_id: Ulid,
    pub digest_item_ids: Vec<Ulid>,
    pub attempts: u32,
    pub next_attempt_at: Timestamp,
    pub state: DeadLetterState,
    pub claim_token: Option<String>,
    /// `Some(DETAIL_SEMANTIC_KIND)` marks a `/digest <ULID>` detail delivery
    /// so the retry worker emits the contract-specific receipt; `None` keeps
    /// the generic `owner.notified` path.
    pub semantic_kind: Option<String>,
    /// Artifact ref of the failure's protected detail (for the receipt's
    /// `refs`); `None` for generic rows or legacy detail rows.
    pub detail_ref: Option<String>,
    /// 1-based page that was attempted; `None` for generic rows.
    pub page_index: Option<i64>,
    /// Total pages for this detail; `None` for generic rows.
    pub page_count: Option<i64>,
    /// `"available"` / `"unavailable:<reason>"` for detail rows; `NULL` for
    /// generic rows.
    pub availability_outcome: Option<String>,
}

pub(super) fn ensure_schema(conn: &rusqlite::Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS digest_items (\n\
           seq INTEGER PRIMARY KEY AUTOINCREMENT,\n\
           id TEXT NOT NULL UNIQUE,\n\
           ts TEXT NOT NULL,\n\
           class TEXT NOT NULL,\n\
           summary TEXT NOT NULL,\n\
           text_ref TEXT,\n\
           resolved INTEGER NOT NULL DEFAULT 0\n\
         );\n\
         CREATE TABLE IF NOT EXISTS notify_dead_letters (\n\
           id TEXT PRIMARY KEY,\n\
           enqueued_at TEXT NOT NULL,\n\
           chat_id INTEGER NOT NULL,\n\
           text_ref TEXT NOT NULL,\n\
           task_grant_id TEXT,\n\
           digest_item_ids TEXT NOT NULL DEFAULT '',\n\
           attempts INTEGER NOT NULL DEFAULT 0,\n\
           next_attempt_at TEXT NOT NULL,\n\
           claimed_until TEXT,\n\
           claim_token TEXT,\n\
           semantic_kind TEXT,\n\
           detail_ref TEXT,\n\
           page_index INTEGER,\n\
           page_count INTEGER,\n\
           availability_outcome TEXT,\n\
           state TEXT NOT NULL DEFAULT 'pending'\n\
         );\n\
         CREATE TABLE IF NOT EXISTS connector_counters (\n\
           connector TEXT NOT NULL,\n\
           outcome TEXT NOT NULL,\n\
           count INTEGER NOT NULL DEFAULT 0,\n\
           PRIMARY KEY (connector, outcome)\n\
         );",
    )?;
    Ok(())
}
