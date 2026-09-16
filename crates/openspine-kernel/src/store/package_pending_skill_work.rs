// The skill ceremony is separate from ordinary grants and owner_reviews.
// Pending mined skills can become installed context after an owner decision;
// installed competence is configuration, not unfinished promotion work.
fn skill_promotion_counts(
    tx: &rusqlite::Transaction<'_>,
) -> Result<(i64, i64, i64), StoreError> {
    use openspine_schemas::skill::{SkillProvenance, SkillState};
    let mut statement = tx.prepare("SELECT state, schema_version, provenance, version FROM skills")?;
    let mut rows = statement.query([])?;
    let mut counts = (0_i64, 0_i64, 0_i64);
    while let Some(row) = rows.next()? {
        let state: String = row.get(0)?;
        let schema: i64 = row.get(1)?;
        let provenance: String = row.get(2)?;
        let version: i64 = row.get(3)?;
        let disposition = if schema != 1 || version <= 0 || u32::try_from(version).is_err() {
            WorkDisposition::Unknown
        } else {
            match (
                serde_json::from_str::<SkillState>(&state),
                serde_json::from_str::<SkillProvenance>(&provenance),
            ) {
                (Ok(SkillState::PendingReview), Ok(SkillProvenance::MinerDistilled)) => WorkDisposition::Outstanding,
                (Ok(SkillState::Installed | SkillState::Rejected | SkillState::Retired), Ok(_)) => WorkDisposition::Terminal,
                _ => WorkDisposition::Unknown,
            }
        };
        count_disposition(&mut counts, disposition)?;
    }
    Ok(counts)
}
