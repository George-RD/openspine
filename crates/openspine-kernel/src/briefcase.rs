//! Kernel-owned briefcase orchestration (AD-021/031/032/121).
use openspine_schemas::action::{ActionId, GateDecision};
use openspine_schemas::briefcase::{
    pack, Briefcase, CounterpartyRef, LearnedSource, PackSources, SectionKind, TaskClass,
    TaskShape, TopUpDecision, TopUpPolicy, TopUpRequest,
};
use openspine_schemas::digest::digest_of;
use openspine_schemas::digest::Digest;
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::identity::{IdentifierKind, RelationshipKind};
use serde_json::{json, Value};
use sha2::Digest as _;
use ulid::Ulid;

#[derive(Debug, Clone, Default)]
pub struct SourcePool {
    pub learned: Vec<LearnedSource>,
}

fn grant_view(grant: &TaskGrant) -> Value {
    json!({
        "schema_version": grant.schema_version,
        "user": grant.user,
        "purpose": grant.purpose,
        "issued_by": grant.issued_by,
        "route_id": grant.route_id,
        "agent_id": grant.agent_id,
        "workflow_id": grant.workflow_id,
        "capability_pack_id": grant.capability_pack_id,
        "persona_id": grant.persona_id,
        "authority_sources": grant.authority_sources,
        "allowed_actions": grant.allowed_actions,
        "approval_required_actions": grant.approval_required_actions,
        "denied_actions": grant.denied_actions,
        "allowed_egress_classes": grant.allowed_egress_classes,
        "output_channels": grant.output_channels,
        "limits": grant.limits,
        "mode": grant.mode,
    })
}

/// Hash a raw email address the same way the identity store hashes
/// identifiers (`identity_identifiers.value_hash`), so a packed email
/// counterparty can be resolved against bound identities without ever
/// storing or comparing the plaintext address.
fn email_address_hash(address: &str) -> Digest {
    let mut hasher = sha2::Sha256::new();
    hasher.update(address.as_bytes());
    openspine_schemas::digest::digest_from_hash(hasher.finalize().into())
}

pub fn pack_for_task(
    grant: &TaskGrant,
    counterparty: CounterpartyRef,
    counterparty_slice: Value,
    class: TaskClass,
    sources: &SourcePool,
) -> Result<Briefcase, BriefcaseKernelError> {
    let tier = counterparty.tier();
    let (preferences, skills) = openspine_schemas::briefcase::select_relevant_sources(
        &sources.learned,
        &grant.workflow_id,
        tier,
    );
    Ok(pack(
        TaskShape {
            route_id: grant.route_id.clone(),
            workflow_id: grant.workflow_id.clone(),
            counterparty,
        },
        &PackSources {
            grant_view: grant_view(grant),
            preferences,
            skills,
            counterparty_slice,
        },
        tier,
        class,
    ))
}

#[allow(dead_code)]
pub fn apply_top_up(
    briefcase: &mut Briefcase,
    request: &TopUpRequest,
    policy: &TopUpPolicy,
    sources: &SourcePool,
) -> Result<TopUpDecision, BriefcaseKernelError> {
    if briefcase
        .top_up_log
        .iter()
        .any(|prior| prior.request.request_id == request.request_id)
    {
        return Err(BriefcaseKernelError::Schema(
            openspine_schemas::briefcase::BriefcaseError::TopUpReplay(request.request_id),
        ));
    }
    let mut decision = briefcase.evaluate_top_up(request, policy);
    if matches!(
        decision.outcome,
        openspine_schemas::briefcase::TopUpOutcome::Allowed
    ) {
        let (preferences, skills) = openspine_schemas::briefcase::select_relevant_sources(
            &sources.learned,
            &briefcase.task_shape.workflow_id,
            briefcase.tier,
        );
        let candidates = match request.kind {
            SectionKind::Preference => preferences,
            SectionKind::Skill => skills,
            _ => Vec::new(),
        };
        let Some(source) = candidates
            .into_iter()
            .find(|s| s.key == request.section_key)
        else {
            decision.outcome = openspine_schemas::briefcase::TopUpOutcome::Denied {
                reason: "source is not relevant to this task shape".into(),
            };
            let mut persisted_decision = decision.clone();
            persisted_decision.request = request.for_persistence();
            briefcase.record_top_up_decision(persisted_decision);
            return Ok(decision);
        };
        decision.source_digest = Some(digest_of(&source.payload));
        briefcase.apply_top_up(decision.clone(), source, policy)?;
        if let Some(recorded) = briefcase.top_up_log.last_mut() {
            recorded.request = request.for_persistence();
        }
    } else {
        let mut persisted_decision = decision.clone();
        persisted_decision.request = request.for_persistence();
        briefcase.record_top_up_decision(persisted_decision);
    }
    Ok(decision)
}

#[allow(dead_code)]
pub fn apply_top_up_for_grant(
    store: &crate::store::Store,
    task_grant_id: Ulid,
    request: &TopUpRequest,
    policy: &TopUpPolicy,
    sources: &SourcePool,
) -> Result<TopUpDecision, BriefcaseKernelError> {
    store.mutate_briefcase(task_grant_id, |briefcase| {
        apply_top_up(briefcase, request, policy, sources)
    })
}

/// Gate-visible top-up: compute the decision against the live briefcase under
/// the transaction's write lock, persist the mutated briefcase, and chain the
/// audit row recording the mutation — all inside one `BEGIN IMMEDIATE`
/// transaction (`Store::mutate_briefcase_and_audit`). This is the production
/// path behind `POST /v1/briefcase/:id/topup`; it never special-cases around
/// the gate (the caller gates first) and never leaves a briefcase update
/// without its audit row.
pub fn apply_top_up_for_grant_atomic(
    store: &crate::store::Store,
    task_grant_id: Ulid,
    request: &TopUpRequest,
    policy: &TopUpPolicy,
    sources: &SourcePool,
    action: &ActionId,
) -> Result<TopUpDecision, BriefcaseKernelError> {
    store.mutate_briefcase_and_audit(task_grant_id, |briefcase| {
        let decision = apply_top_up(briefcase, request, policy, sources)?;
        let audit = if matches!(
            decision.outcome,
            openspine_schemas::briefcase::TopUpOutcome::Allowed
        ) {
            crate::store::briefcase_support::BriefcaseAudit {
                kind: "briefcase.topup.applied".to_string(),
                action: Some(action.clone()),
                decision: Some(GateDecision::Allow),
                reason: None,
                task_grant_id: Some(task_grant_id),
                target_refs: vec![],
                payload_refs: vec![],
            }
        } else {
            crate::store::briefcase_support::BriefcaseAudit {
                kind: "briefcase.topup.denied".to_string(),
                action: Some(action.clone()),
                decision: Some(GateDecision::Deny {
                    reason: openspine_schemas::action::DenialReason::ExplicitDeny,
                }),
                reason: Some("top-up denied".to_string()),
                task_grant_id: Some(task_grant_id),
                target_refs: vec![],
                payload_refs: vec![],
            }
        };
        Ok((decision, audit))
    })
}

#[derive(Debug, thiserror::Error)]
pub enum BriefcaseKernelError {
    #[error("briefcase schema operation failed: {0}")]
    Schema(#[from] openspine_schemas::briefcase::BriefcaseError),
    #[error("briefcase store operation failed: {0}")]
    Store(#[from] crate::store::StoreError),
    #[error("kernel source {0:?} is unavailable")]
    SourceUnavailable(String),
}

/// Compose the briefcase at the Grant→Run boundary and return it WITHOUT
/// persisting it. The caller is responsible for persisting the grant and the
/// briefcase atomically (see `Store::insert_grant_and_briefcase_atomic`) so a
/// crash between the two writes cannot strand an orphan grant (D-050).
pub async fn pack_for_pipeline(
    state: &crate::pipeline::AppState,
    _thread_id: Option<&str>,
    lane: openspine_schemas::event::Lane,
    grant: &TaskGrant,
    counterparty_address: Option<&str>,
) -> Result<Briefcase, BriefcaseKernelError> {
    let counterparty = match lane {
        openspine_schemas::event::Lane::OwnerControl => CounterpartyRef::Bound {
            identity_id: state.owner_identity_id,
            relationship: openspine_schemas::identity::RelationshipKind::Owner,
        },
        // AD-134 headless hook lane: the counterparty is the hook id
        // carried on the lane, never a private address. No owner-bound
        // relationship is asserted for a working-machinery event.
        openspine_schemas::event::Lane::ExternalCommunication => {
            // The email-lane counterparty is the thread's newest non-owner
            // recipient. The address MUST be carried from the authorized preflight
            // snapshot (a catalogued, header-only Gmail read) — it is never
            // derived from the thread id (a thread id is an event Ulid, not a
            // person). A missing address is a packing failure, never a silent
            // "unavailable" placeholder (AD-021 truthfulness).
            let address = counterparty_address.ok_or_else(|| {
                BriefcaseKernelError::SourceUnavailable(
                    "email counterparty address not carried from authorized preflight snapshot"
                        .into(),
                )
            })?;
            let hash = email_address_hash(address);
            match state
                .store
                .resolve_identity_by_identifier_hash(&hash, IdentifierKind::Email)?
            {
                Some(identity) => {
                    let relationship = identity
                        .relationships
                        .iter()
                        .find(|r| r.target_id == state.owner_identity_id)
                        .map(|r| r.kind)
                        .unwrap_or(RelationshipKind::Unknown);
                    CounterpartyRef::Bound {
                        identity_id: identity.id,
                        relationship,
                    }
                }
                None => {
                    let digest_str = format!(
                        "email:{}",
                        hash.as_str()
                            .strip_prefix("sha256:")
                            .unwrap_or(hash.as_str())
                    );
                    CounterpartyRef::Unresolved {
                        channel: "email".to_string(),
                        identifier: digest_str,
                    }
                }
            }
        }
        // Every other lane (headless hooks included) is working machinery
        // with no private counterparty relationship. The hook id travels as
        // the unresolved counterparty identifier so the briefcase is auditable.
        _ => CounterpartyRef::Unresolved {
            channel: "webhook".to_string(),
            identifier: counterparty_address.unwrap_or("unknown-hook").to_string(),
        },
    };
    let class = if lane == openspine_schemas::event::Lane::OwnerControl {
        TaskClass::Conversation
    } else {
        TaskClass::DraftApproval
    };
    let counterparty_slice = match &counterparty {
        CounterpartyRef::Bound { identity_id, .. } => state
            .store
            .get_identity(*identity_id)?
            .map(|identity| {
                json!({
                    "identity_id": identity.id,
                    "display_name": identity.display_name,
                    "entity_type": identity.entity_type
                })
            })
            .unwrap_or_else(|| json!({"identity_id": *identity_id})),
        CounterpartyRef::Unresolved {
            channel,
            identifier,
        } => json!({
            "channel": channel,
            "identifier": identifier
        }),
    };
    let source_pool = SourcePool {
        learned: state.store.list_learned_sources()?,
    };
    pack_for_task(grant, counterparty, counterparty_slice, class, &source_pool)
}

#[cfg(test)]
mod tests;
