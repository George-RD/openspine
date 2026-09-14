use crate::artifact_store::ArtifactStore;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::Digest;
use openspine_schemas::owner_review::OwnerReviewRequest;

impl Store {
    /// Offline package maintenance: the caller holds the data-root lifetime
    /// lock. SQL counts share one read transaction; canonical review blobs are
    /// verified without recovery. The returned counts own no live references.
    pub(crate) fn package_outstanding_work_with_reviews(
        &self,
        as_of: Timestamp,
        artifacts: &ArtifactStore,
    ) -> Result<PackageOutstandingWork, StoreError> {
        self.capture_package_outstanding_work(as_of, Some(artifacts))
    }

    // Bare Store fixtures have no artifact store. There is deliberately no
    // production entry point that can waive canonical review evidence.
    #[cfg(test)]
    pub(crate) fn package_outstanding_work(
        &self,
        as_of: Timestamp,
    ) -> Result<PackageOutstandingWork, StoreError> {
        self.capture_package_outstanding_work(as_of, None)
    }
}

struct ReviewProposal {
    kind: String,
    artifact_id: String,
    version: i64,
    yaml_digest: String,
    task_grant_id: String,
    action_request_id: Option<String>,
}

fn proposal_work_counts(
    tx: &rusqlite::Transaction<'_>,
    as_of: Timestamp,
    artifacts: Option<&ArtifactStore>,
) -> Result<(i64, i64, i64), StoreError> {
    let mut statement = tx.prepare(
        "SELECT state, kind, artifact_id, version, yaml_digest,
                task_grant_id, action_request_id FROM proposed_artifacts",
    )?;
    let mut rows = statement.query([])?;
    let mut counts = (0_i64, 0_i64, 0_i64);
    while let Some(row) = rows.next()? {
        let state: String = row.get(0)?;
        let disposition = match state.as_str() {
            "proposed" | "validated" | "approved" => WorkDisposition::Outstanding,
            "active" | "quarantined" | "retired" => WorkDisposition::Terminal,
            "review_required" => match artifacts {
                Some(artifacts) => review_required_disposition(
                    tx,
                    &ReviewProposal {
                        kind: row.get(1)?,
                        artifact_id: row.get(2)?,
                        version: row.get(3)?,
                        yaml_digest: row.get(4)?,
                        task_grant_id: row.get(5)?,
                        action_request_id: row.get(6)?,
                    },
                    as_of,
                    artifacts,
                )?,
                None => WorkDisposition::Unknown,
            },
            _ => WorkDisposition::Unknown,
        };
        count_disposition(&mut counts, disposition)?;
    }
    Ok(counts)
}

fn review_required_disposition(
    tx: &rusqlite::Transaction<'_>,
    proposal: &ReviewProposal,
    as_of: Timestamp,
    artifacts: &ArtifactStore,
) -> Result<WorkDisposition, StoreError> {
    let Some(request_id) = proposal.action_request_id.as_deref() else {
        return Ok(WorkDisposition::Unknown);
    };
    let request_row: Option<(String, i64)> = tx
        .query_row(
            "SELECT request_json, used FROM action_requests WHERE id = ?1",
            [request_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((json, used)) = request_row else {
        return Ok(WorkDisposition::Unknown);
    };
    let catalog = crate::action_catalog::canonical_catalog();
    let Some(request) = census_request(request_id, &json, &catalog) else {
        return Ok(WorkDisposition::Unknown);
    };
    let references: i64 = tx.query_row(
        "SELECT COUNT(*) FROM proposed_artifacts WHERE action_request_id = ?1",
        [request_id],
        |row| row.get(0),
    )?;
    if references != 1
        || proposal.version <= 0
        || u32::try_from(proposal.version).is_err()
        || proposal.artifact_id.is_empty()
        || request.action.as_str() != "artifact.activate"
        || request.task_grant_id.to_string() != proposal.task_grant_id
        || request.payload_ref.as_ref().map(|value| value.digest.as_str())
            != Some(proposal.yaml_digest.as_str())
    {
        return Ok(WorkDisposition::Unknown);
    }
    // A terminal semantic review cannot waive a still-live ordinary callback.
    // Conversely, an expired ordinary grant cannot waive a live evaluated
    // review: that path may mint fresh activation authority at decision time.
    let ordinary = match used {
        0 => request_disposition(tx, &request, as_of)?,
        1 => WorkDisposition::Terminal,
        _ => WorkDisposition::Unknown,
    };
    if ordinary != WorkDisposition::Terminal {
        return Ok(ordinary);
    }
    matching_review_disposition(tx, proposal, as_of, artifacts)
}

fn matching_review_disposition(
    tx: &rusqlite::Transaction<'_>,
    proposal: &ReviewProposal,
    as_of: Timestamp,
    artifacts: &ArtifactStore,
) -> Result<WorkDisposition, StoreError> {
    let source_json: Option<String> = tx
        .query_row(
            "SELECT grant_json FROM task_grants WHERE id = ?1",
            [&proposal.task_grant_id],
            |row| row.get(0),
        )
        .optional()?;
    let source = match source_json {
        Some(json) => match serde_json::from_str::<TaskGrant>(&json) {
            Ok(grant) => Some(grant),
            Err(_) => return Ok(WorkDisposition::Unknown),
        },
        None => None,
    };
    let mut statement = tx.prepare(
        "SELECT id, artifact_ref_digest, artifact_ref_schema_version,
                state, expires_at, owner_principal_id FROM owner_reviews",
    )?;
    let mut rows = statement.query([])?;
    let (mut matched, mut live, mut unknown) = (false, false, false);
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let digest: String = row.get(1)?;
        let schema: i64 = row.get(2)?;
        let state: String = row.get(3)?;
        let expires_at: i64 = row.get(4)?;
        let principal: String = row.get(5)?;
        let Some(review) = read_canonical_review(artifacts, &id, digest, schema) else {
            // SQL holds no proposal binding. Unreadable bytes cannot be
            // assumed unrelated to the proposal currently being classified.
            unknown = true;
            continue;
        };
        let Some(binding) = review.evaluation_binding.as_ref() else {
            continue;
        };
        if binding.artifact_kind != proposal.kind
            || binding.artifact_id != proposal.artifact_id
            || i64::from(binding.artifact_version) != proposal.version
        {
            continue;
        }
        matched = true;
        if binding.artifact_kind != "standing_rule"
            || binding.action_request_id.to_string().as_str()
                != proposal.action_request_id.as_deref().unwrap_or_default()
            || binding.proposal_digest.as_str() != proposal.yaml_digest
            || review.proposal_digest != binding.proposal_digest
            || principal.parse::<ulid::Ulid>().is_err()
            || source.as_ref().is_some_and(|grant| grant.user.to_string() != principal)
        {
            unknown = true;
            continue;
        }
        match state.as_str() {
            "pending" => {
                live |= i128::from(expires_at) > as_of.as_nanosecond();
            }
            // Evaluated activation commits require a pending review. A
            // narrowed original is superseded; its replacement proposal and
            // any other live exact review are counted independently.
            "narrowed" | "rejected" | "revoked" | "expired" => {}
            // An approved review with an unactivated proposal is not proof
            // that activation work is finished; preserve the inconsistency.
            _ => unknown = true,
        }
    }
    Ok(if unknown {
        WorkDisposition::Unknown
    } else if live || !matched {
        WorkDisposition::Outstanding
    } else {
        WorkDisposition::Terminal
    })
}

fn read_canonical_review(
    artifacts: &ArtifactStore,
    id: &str,
    digest: String,
    schema: i64,
) -> Option<OwnerReviewRequest> {
    if schema != 1 {
        return None;
    }
    let artifact_ref = ArtifactRef {
        digest: Digest::parse(digest).ok()?,
        schema_version: 1,
    };
    let bytes = artifacts
        .get_scoped_without_recovery(crate::counterparty_keys::SYSTEM_SCOPE, &artifact_ref)
        .ok()?;
    let review: OwnerReviewRequest = serde_json::from_slice(&bytes).ok()?;
    (review.id.to_string() == id
        && review.schema_version == 1
        && review.review_version > 0
        && review.binding_is_valid())
        .then_some(review)
}
