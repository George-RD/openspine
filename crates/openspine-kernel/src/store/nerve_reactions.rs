//! Durable, issuance-authenticated reaction mining hook for nerves.

use super::Store;
use openspine_schemas::nerve::{
    InterjectionReaction, NerveError, NerveInterjection, NerveType, IGNORED_RETIRE_THRESHOLD,
};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use ulid::Ulid;

#[allow(dead_code)]
fn class_digest(class: &str) -> String {
    openspine_schemas::digest::digest_of_bytes(class.as_bytes()).to_string()
}

impl Store {
    /// Return whether an opaque interjection class has crossed the durable
    /// five-ignore retirement threshold.
    #[allow(dead_code)]
    pub(crate) fn class_retired(&self, nerve_id: Ulid, class: &str) -> Result<bool, NerveError> {
        let conn = self.conn.lock();
        let retired: Option<i64> = conn
            .query_row(
                "SELECT retired FROM nerve_decay WHERE nerve_id = ?1 AND class = ?2",
                params![nerve_id.to_string(), class_digest(class)],
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| NerveError::Storage(err.to_string()))?;
        Ok(retired == Some(1))
    }

    /// Record a reaction through the recorded-only mining hook. All three
    /// signals are durable; only `Ignored` advances the retirement counter.
    /// The issuance row authenticates the emitted id and class digest, and the
    /// unique reaction row makes retries idempotent.
    #[allow(dead_code)]
    pub(crate) fn record_reaction(
        &self,
        nerve_id: Ulid,
        interjection: &NerveInterjection,
        reaction: InterjectionReaction,
    ) -> Result<(), NerveError> {
        let class = match interjection.nerve_type {
            NerveType::Advisor => interjection
                .advisor
                .as_ref()
                .ok_or(NerveError::InvalidPayload)?
                .concern_class
                .as_str(),
            NerveType::Screener => interjection
                .screener
                .as_ref()
                .ok_or(NerveError::InvalidPayload)?
                .manipulation_class
                .as_str(),
            _ => interjection.class.as_str(),
        };
        if class.is_empty() || class != interjection.class {
            return Err(NerveError::InvalidPayload);
        }
        let mut conn = self.conn.lock();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|err| NerveError::Storage(err.to_string()))?;
        let issuance: Option<(String, String, String, String)> = tx
            .query_row(
                "SELECT nerve_id, advisee_id, nerve_type, class_digest
                 FROM nerve_issuances WHERE interjection_id = ?1",
                params![interjection.id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(|err| NerveError::Storage(err.to_string()))?;
        let Some((issued_nerve, issued_advisee, issued_type, issued_digest)) = issuance else {
            return Err(NerveError::InvalidPayload);
        };
        let expected_type = serde_json::to_string(&interjection.nerve_type)
            .map_err(|err| NerveError::Storage(err.to_string()))?;
        if issued_nerve != nerve_id.to_string()
            || issued_advisee != interjection.advisee_id
            || issued_type != expected_type
            || issued_digest != class_digest(class)
        {
            return Err(NerveError::InvalidPayload);
        }
        let reaction_name = match reaction {
            InterjectionReaction::Ignored => "ignored",
            InterjectionReaction::Engaged => "engaged",
            InterjectionReaction::Annoyed => "annoyed",
        };
        let inserted = tx
            .execute(
                "INSERT OR IGNORE INTO nerve_reactions (interjection_id, reaction)
                 VALUES (?1, ?2)",
                params![interjection.id.to_string(), reaction_name],
            )
            .map_err(|err| NerveError::Storage(err.to_string()))?;
        if inserted == 0 {
            tx.commit()
                .map_err(|err| NerveError::Storage(err.to_string()))?;
            return Ok(());
        }
        let (ignored, engaged, annoyed) = match reaction {
            InterjectionReaction::Ignored => (1_i64, 0_i64, 0_i64),
            InterjectionReaction::Engaged => (0_i64, 1_i64, 0_i64),
            InterjectionReaction::Annoyed => (0_i64, 0_i64, 1_i64),
        };
        tx.execute(
            "INSERT INTO nerve_decay
             (nerve_id, class, ignored_count, retired, engaged_count, annoyed_count)
             VALUES (?1, ?2, ?3, CASE WHEN ?3 >= ?6 THEN 1 ELSE 0 END, ?4, ?5)
             ON CONFLICT(nerve_id, class) DO UPDATE SET
               ignored_count = nerve_decay.ignored_count + excluded.ignored_count,
               engaged_count = nerve_decay.engaged_count + excluded.engaged_count,
               annoyed_count = nerve_decay.annoyed_count + excluded.annoyed_count,
               retired = CASE WHEN nerve_decay.ignored_count + excluded.ignored_count >= ?6
                              THEN 1 ELSE nerve_decay.retired END",
            params![
                nerve_id.to_string(),
                class_digest(class),
                ignored,
                engaged,
                annoyed,
                i64::from(IGNORED_RETIRE_THRESHOLD)
            ],
        )
        .map_err(|err| NerveError::Storage(err.to_string()))?;
        tx.commit()
            .map_err(|err| NerveError::Storage(err.to_string()))?;
        Ok(())
    }
}
