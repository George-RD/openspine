use openspine_schemas::artifact::{ProposalApprovalEvidence, ProposalApprovalPath};

/// Called only after the ordinary request is terminal and no canonical
/// semantic review matched. Absence of a review is not itself evidence that
/// fresh authority was impossible; use the kernel's original audit receipt.
fn unmatched_proposal_disposition(
    tx: &rusqlite::Transaction<'_>,
    proposal: &ReviewProposal,
) -> Result<WorkDisposition, StoreError> {
    let Ok(proposal_id) = proposal.id.parse::<ulid::Ulid>() else {
        return Ok(WorkDisposition::Unknown);
    };
    let Some(request_id) = proposal.action_request_id.as_deref() else {
        return Ok(WorkDisposition::Unknown);
    };
    let Ok(request_id) = request_id.parse::<ulid::Ulid>() else {
        return Ok(WorkDisposition::Unknown);
    };
    let Ok(grant_id) = proposal.task_grant_id.parse::<ulid::Ulid>() else {
        return Ok(WorkDisposition::Unknown);
    };
    let mut evidence = None;
    let mut callback_delivered = false;
    let mut unknown = false;
    let filter = EventSubscriptionFilter::kinds([
        AuditKind::from_static("artifact.proposed"),
        AuditKind::from_static("artifact.proposal_callback_delivered"),
    ]);
    Store::visit_audit_conn(tx, &filter, 0, |entry| {
        let event = entry.event;
        let Some(json) = event.payload_json.as_deref() else {
            // Legacy records did not identify the decision path. Do not
            // rewrite history or infer ordinary approval from missing data.
            return Ok(());
        };
        let proof: ProposalApprovalEvidence = match serde_json::from_str(json) {
            Ok(proof) => proof,
            Err(_) => {
                if event.task_grant_id == Some(grant_id)
                    && event
                        .payload_refs
                        .iter()
                        .any(|value| value.digest.as_str() == proposal.yaml_digest)
                {
                    unknown = true;
                }
                return Ok(());
            }
        };
        if proof.proposal_id != proposal_id && proof.action_request_id != request_id {
            return Ok(());
        }
        if proof.schema_version != 1
            || proof.proposal_id != proposal_id
            || proof.action_request_id != request_id
            || proof.task_grant_id != grant_id
            || proof.artifact_kind != proposal.kind
            || proof.artifact_id != proposal.artifact_id
            || i64::from(proof.artifact_version) != proposal.version
            || proof.proposal_digest.as_str() != proposal.yaml_digest
            || event.action.as_ref().map(|action| action.as_str()) != Some("artifact.propose")
            || event.task_grant_id != Some(grant_id)
            || event.payload_refs.len() != 1
            || event.payload_refs[0].schema_version != 1
            || event.payload_refs[0].digest != proof.proposal_digest
        {
            unknown = true;
            return Ok(());
        }
        match event.kind.as_str() {
            "artifact.proposed" => {
                if evidence.is_some() {
                    unknown = true;
                } else {
                    evidence = Some(proof.approval_path);
                }
            }
            "artifact.proposal_callback_delivered" => {
                // Delivery evidence is meaningful only for the ordinary
                // callback path and only after its exact proposed receipt.
                if callback_delivered
                    || proof.approval_path != ProposalApprovalPath::GrantBoundCallback
                    || evidence != Some(ProposalApprovalPath::GrantBoundCallback)
                {
                    unknown = true;
                }
                callback_delivered = true;
            }
            _ => unreachable!("filter admits only proposal lifecycle evidence"),
        }
        Ok(())
    })?;
    Ok(if unknown {
        WorkDisposition::Unknown
    } else if evidence == Some(ProposalApprovalPath::GrantBoundCallback) {
        if callback_delivered {
            WorkDisposition::Terminal
        } else {
            // A persisted callback request that was never proven delivered
            // cannot be silently cleared merely because its grant expired.
            WorkDisposition::Unknown
        }
    } else {
        // An evaluated path can still issue fresh authority, even after
        // losing the ordinary grant. Missing legacy evidence stays blocking.
        WorkDisposition::Outstanding
    })
}
