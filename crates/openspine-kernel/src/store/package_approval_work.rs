use openspine_schemas::action::{ActionCatalog, ActionRequest};
use openspine_schemas::grant::TaskGrant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkDisposition {
    Outstanding,
    Terminal,
    Unknown,
}

fn census_grant(id: &str, index: &str, json: &str) -> Option<TaskGrant> {
    let indexed_expiry = index.parse::<Timestamp>().ok()?;
    let grant: TaskGrant = serde_json::from_str(json).ok()?;
    (grant.schema_version == 1
        && grant.id.to_string() == id
        && grant.expires_at == indexed_expiry)
        .then_some(grant)
}

fn census_request(id: &str, json: &str, catalog: &ActionCatalog) -> Option<ActionRequest> {
    let request: ActionRequest = serde_json::from_str(json).ok()?;
    (request.schema_version == 1
        && request.id.to_string() == id
        && catalog.contains(&request.action)
        && request.payload_ref.as_ref().is_some_and(|payload| payload.schema_version == 1)
        && request.target_digest.is_some())
        .then_some(request)
}

fn count_disposition(
    counts: &mut (i64, i64, i64),
    disposition: WorkDisposition,
) -> Result<(), StoreError> {
    counts.0 = checked_add_i64(counts.0, 1)?;
    match disposition {
        WorkDisposition::Outstanding => counts.1 = checked_add_i64(counts.1, 1)?,
        WorkDisposition::Terminal => counts.2 = checked_add_i64(counts.2, 1)?,
        WorkDisposition::Unknown => {}
    }
    Ok(())
}

fn action_request_counts(
    tx: &rusqlite::Transaction<'_>,
    as_of: Timestamp,
) -> Result<(i64, i64, i64), StoreError> {
    let catalog = crate::action_catalog::canonical_catalog();
    let mut statement = tx.prepare("SELECT id, request_json, used FROM action_requests")?;
    let mut rows = statement.query([])?;
    let mut counts = (0_i64, 0_i64, 0_i64);
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let json: String = row.get(1)?;
        let used: i64 = row.get(2)?;
        let disposition = match used {
            // Consumption is durable even when the old payload is no longer
            // readable. Any unfinished effect has its own fence/work row.
            1 => WorkDisposition::Terminal,
            0 => match census_request(&id, &json, &catalog) {
                Some(request) => request_disposition(tx, &request, as_of)?,
                None => WorkDisposition::Unknown,
            },
            _ => WorkDisposition::Unknown,
        };
        count_disposition(&mut counts, disposition)?;
    }
    Ok(counts)
}

fn request_disposition(
    tx: &rusqlite::Transaction<'_>,
    request: &ActionRequest,
    as_of: Timestamp,
) -> Result<WorkDisposition, StoreError> {
    // Reconfirmation can mint or refresh its reserved grant at decision time.
    // Ordinary callbacks cannot. Evaluated owner reviews have a separate
    // fresh-authority path, counted independently by the review/proposal census.
    if request.action.as_str() == "artifact.reconfirm" {
        return Ok(WorkDisposition::Outstanding);
    }
    let id = request.task_grant_id.to_string();
    let row: Option<(String, String)> = tx
        .query_row(
            "SELECT expires_at, grant_json FROM task_grants WHERE id = ?1",
            [&id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(match row {
        None => WorkDisposition::Terminal,
        Some((index, json)) => match census_grant(&id, &index, &json) {
            Some(grant) if grant.is_expired(as_of) => WorkDisposition::Terminal,
            Some(_) => WorkDisposition::Outstanding,
            None => WorkDisposition::Unknown,
        },
    })
}

include!("package_review_work.rs");

#[cfg(test)]
mod review_work_tests {
    include!("package_review_work_tests.rs");
}
