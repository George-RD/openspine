//! Deterministic disclosure-policy egress gate.

use std::collections::BTreeSet;

use jiff::Timestamp;
use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::{ArtifactRef, Lifecycle};
use openspine_schemas::briefcase::{BriefcaseSection, VisibilityClass};
use openspine_schemas::disclosure_policy::{
    check_egress, generalize_query, ClassifiedBriefcaseItem, DisclosureCarveOut, DisclosureClass,
    DisclosurePolicy, DisclosurePolicyKey, DisclosureProvenance, OutboundQuery,
    OwnerQuestionEscalation, PreparedQuery, PreparedQueryRef,
};
use openspine_schemas::egress::EgressClass;
use openspine_schemas::escalation::EscalationEvent;
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::identity::RelationshipKind;
use openspine_schemas::standing_rule::{BudgetWindow, StandingRuleManifest};
use serde_json::Value;
use ulid::Ulid;

use crate::action_catalog::canonical_catalog;
use crate::escalation::route_escalation;
use crate::pipeline::AppState;
use crate::store::{Store, StoreError};

#[allow(dead_code)]
pub(crate) struct DisclosureRequest {
    pub raw_query: String,
    pub sensitive_terms: BTreeSet<String>,
    pub action_id: ActionId,
    pub relationship: RelationshipKind,
    pub provenance: DisclosureProvenance,
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum DisclosureError {
    Blocked(OwnerQuestionEscalation),
    UnratedEgress(ActionId),
    UnclassifiedSection(String),
    BudgetExhausted(String),
    Store(StoreError),
}

pub(crate) type DisclosureReservation = (String, u32, String);

pub(crate) struct EnforcedDisclosure {
    pub query: OutboundQuery,
    pub reservations: Vec<DisclosureReservation>,
}

fn action_for_scope(key: DisclosurePolicyKey, class: EgressClass) -> ActionId {
    ActionId::new(format!(
        "disclosure.egress:{}:{}:{}",
        egress_slug(class),
        relationship_slug(key.relationship),
        disclosure_slug(key.disclosure_class)
    ))
}

#[allow(dead_code)]
fn action_for_egress(class: EgressClass) -> ActionId {
    action_for_scope(
        DisclosurePolicyKey {
            relationship: RelationshipKind::Unknown,
            disclosure_class: DisclosureClass::Public,
        },
        class,
    )
}

fn trusted_egress_class(action_id: &ActionId) -> Option<EgressClass> {
    canonical_catalog()
        .egress_decl_for(action_id)
        .and_then(|decl| decl.egress_class)
}

fn cancel_reservations(store: &Store, reservations: &[DisclosureReservation]) {
    for (_, _, reservation_id) in reservations {
        if let Err(err) = store.cancel_standing_rule_reservation(reservation_id) {
            tracing::error!(error = %err, reservation_id, "disclosure reservation cancel failed");
        }
    }
}

/// Enforce disclosure policy and reserve the scoped D-107 envelope budget.
/// Reservations are returned to dispatch and finalized only after the effect
/// succeeds; a blocked or failed pre-effect path cancels them.
pub(crate) async fn enforce_disclosure_egress(
    state: &AppState,
    grant: &TaskGrant,
    request: DisclosureRequest,
) -> Result<EnforcedDisclosure, DisclosureError> {
    let egress_class = trusted_egress_class(&request.action_id)
        .ok_or_else(|| DisclosureError::UnratedEgress(request.action_id.clone()))?;
    let query = OutboundQuery::from_private_context(
        &request.raw_query,
        &request.sensitive_terms,
        egress_class,
        request.provenance,
    );
    let now = Timestamp::now();
    let all_policies = state
        .store
        .load_disclosure_policies()
        .map_err(DisclosureError::Store)?;
    let mut reservations = Vec::new();
    let mut policies = Vec::new();
    for policy in all_policies {
        if policy.key.relationship != request.relationship
            || !query
                .provenance
                .classes()
                .contains(&policy.key.disclosure_class)
        {
            continue;
        }
        let Some(rule_id) = policy.standing_rule_bindings.get(&egress_class) else {
            continue;
        };
        let consulted = match state
            .store
            .consult_and_reserve_standing_rule(&action_for_scope(policy.key, egress_class), now)
        {
            Ok(consulted) => consulted,
            Err(err) => {
                // All-or-nothing: a later class's consult/reserve failing
                // must not leave earlier classes' envelopes holding budget
                // for a request that as a whole never went through.
                cancel_reservations(&state.store, &reservations);
                return Err(DisclosureError::Store(err));
            }
        };
        let Some((rule, reservation)) = consulted else {
            continue;
        };
        let Some(reservation_id) = reservation else {
            if &rule.rule_id == rule_id {
                // Active covering envelope with exhausted quota/rate is a
                // budget condition, not a missing policy: the owner already
                // approved this scope, and re-answering the same version
                // cannot reset a time-windowed budget, so this must never
                // mint a new "/disclosure allow" owner question. Fail closed
                // honestly and let the caller retry once the window resets.
                cancel_reservations(&state.store, &reservations);
                // Distinct honest surface: the exhaustion is owner-visible in
                // the audit ledger even though the worker only sees a generic
                // retry-after denial.
                state
                    .store
                    .append_audit(
                        "disclosure.budget_exhausted",
                        Some(&request.action_id),
                        None,
                        Some(&rule.rule_id),
                        Some(grant.id),
                        &[],
                        &[],
                    )
                    .map_err(DisclosureError::Store)?;
                return Err(DisclosureError::BudgetExhausted(rule.rule_id));
            }
            continue;
        };
        if &rule.rule_id == rule_id {
            reservations.push((rule.rule_id, rule.version, reservation_id));
            policies.push(policy);
        } else {
            cancel_reservations(
                &state.store,
                &[(rule.rule_id, rule.version, reservation_id)],
            );
        }
    }
    let blocked_query_digest =
        openspine_schemas::digest::digest_of_bytes(query.generalized_query.as_bytes());
    match check_egress(request.relationship, query, &policies) {
        openspine_schemas::disclosure_policy::DisclosureGateDecision::Allow { query } => {
            Ok(EnforcedDisclosure {
                query,
                reservations,
            })
        }
        openspine_schemas::disclosure_policy::DisclosureGateDecision::Block { escalation } => {
            cancel_reservations(&state.store, &reservations);
            let pending_id = Ulid::new();
            state
                .store
                .store_disclosure_pending_question(
                    &pending_id,
                    grant.id,
                    escalation.key.relationship,
                    escalation.key.disclosure_class,
                    escalation.egress_class,
                    blocked_query_digest,
                    now,
                )
                .map_err(DisclosureError::Store)?;
            let event = EscalationEvent::owner_question(
                grant.id,
                format!(
                    "Disclosure requires owner decision. Reply '/disclosure allow {}', '/disclosure allow-with-carve-out {}', or '/disclosure deny {}'",
                    pending_id, pending_id, pending_id
                ),
                grant.thread_id.clone(),
                now,
            );
            route_escalation(state, grant, &event)
                .await
                .map_err(DisclosureError::Store)?;
            Err(DisclosureError::Blocked(escalation))
        }
    }
}

fn collect_nested_strings(value: &Value, terms: &mut BTreeSet<String>) {
    match value {
        Value::String(value) if !value.is_empty() => {
            terms.insert(value.clone());
        }
        Value::Array(values) => values
            .iter()
            .for_each(|value| collect_nested_strings(value, terms)),
        Value::Object(values) => values
            .values()
            .for_each(|value| collect_nested_strings(value, terms)),
        _ => {}
    }
}

fn sensitive_terms_from_sections(sections: &[BriefcaseSection]) -> BTreeSet<String> {
    let mut terms = BTreeSet::new();
    for section in sections {
        if matches!(
            section.disclosure_class,
            Some(DisclosureClass::Private | DisclosureClass::Sensitive)
        ) {
            collect_nested_strings(&section.payload, &mut terms);
        }
    }
    terms
}

/// Kernel-derived provenance for every non-public classified section that can
/// reach a worker's view — Internal, Private, and Sensitive all require a
/// covering policy (`DisclosureClass::requires_policy`); only Public is
/// exempt. `KernelBound` sections never reach a worker's context and are
/// excluded from query provenance. Any worker-visible section WITHOUT a
/// disclosure classification fails closed: legacy/unknown content must never
/// silently drop out of the enforced set.
pub(crate) fn provenance_from_sections(
    sections: &[BriefcaseSection],
) -> Result<DisclosureProvenance, DisclosureError> {
    let mut items = Vec::new();
    for section in sections {
        if matches!(section.visibility, VisibilityClass::KernelBound) {
            continue;
        }
        let Some(disclosure_class) = section.disclosure_class else {
            return Err(DisclosureError::UnclassifiedSection(section.key.clone()));
        };
        if !disclosure_class.requires_policy() {
            continue;
        }
        items.push(ClassifiedBriefcaseItem {
            item_ref: ArtifactRef {
                digest: openspine_schemas::digest::digest_of(&section.payload),
                schema_version: 1,
            },
            disclosure_class,
        });
    }
    Ok(DisclosureProvenance { items })
}

pub(crate) async fn prepare_disclosure_query(
    state: &AppState,
    grant_id: Ulid,
    action_id: ActionId,
    raw_query: String,
    relationship: RelationshipKind,
    egress_class: EgressClass,
    sections: &[BriefcaseSection],
) -> Result<PreparedQueryRef, DisclosureError> {
    let kernel_provenance = provenance_from_sections(sections)?;
    let sensitive_terms = sensitive_terms_from_sections(sections);
    let generalized_query = generalize_query(&raw_query, &sensitive_terms);
    let digest = openspine_schemas::digest::digest_of_bytes(
        format!(
            "{}|{}|{:?}|{:?}|{}",
            grant_id, action_id, relationship, egress_class, generalized_query
        )
        .as_bytes(),
    );
    let prepared = PreparedQuery {
        id: format!("prepared:{}", Ulid::new()),
        grant_id,
        action_id,
        relationship,
        egress_class,
        provenance: kernel_provenance,
        generalized_query,
        digest,
        created_at: Timestamp::now(),
    };
    state
        .store
        .store_prepared_query(&prepared)
        .map_err(DisclosureError::Store)?;
    Ok(PreparedQueryRef {
        id: prepared.id,
        digest: prepared.digest,
    })
}

#[allow(dead_code)]
pub(crate) fn record_owner_answer(
    store: &Store,
    key: DisclosurePolicyKey,
    egress_class: EgressClass,
    carve_outs: Vec<DisclosureCarveOut>,
    now: Timestamp,
) -> Result<DisclosurePolicy, StoreError> {
    let action = action_for_scope(key, egress_class);
    let rule_id = format!(
        "disclosure:{}:{}:{}",
        relationship_slug(key.relationship),
        disclosure_slug(key.disclosure_class),
        egress_slug(egress_class)
    );
    let current_version = store.standing_rule_version_for_action(&action)?;
    let active_version = store
        .active_standing_rule_for_action(&action, now)?
        .map(|rule| rule.version);
    let version = active_version
        .or_else(|| current_version.map(|v| v.saturating_add(1)))
        .unwrap_or(1);
    let manifest = StandingRuleManifest {
        id: rule_id.clone(),
        schema_version: 1,
        version,
        lifecycle_state: Lifecycle::Active,
        action_id: action,
        description: "Owner disclosure envelope".to_string(),
        quota: BudgetWindow {
            max: 20,
            window_secs: 604_800,
        },
        rate: BudgetWindow {
            max: 5,
            window_secs: 3_600,
        },
        expires_after_secs: 7_776_000,
        dark_window: None,
    };
    store.activate_standing_rule(&manifest, None, now)?;
    let policy_id = format!(
        "disclosure:{}:{}",
        relationship_slug(key.relationship),
        disclosure_slug(key.disclosure_class)
    );
    let existing = store
        .load_disclosure_policies()?
        .into_iter()
        .find(|policy| policy.id == policy_id);
    let mut allowed = existing
        .as_ref()
        .map(|policy| policy.allowed_egress_classes.clone())
        .unwrap_or_default();
    if carve_outs.is_empty() && !allowed.contains(&egress_class) {
        allowed.push(egress_class);
    }
    let mut bindings = existing
        .as_ref()
        .map(|policy| policy.standing_rule_bindings.clone())
        .unwrap_or_default();
    bindings.insert(egress_class, rule_id);
    let mut merged_carve_outs = existing
        .as_ref()
        .map(|policy| policy.carve_outs.clone())
        .unwrap_or_default();
    merged_carve_outs.extend(carve_outs);
    let policy = DisclosurePolicy {
        id: policy_id,
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        key,
        allowed_egress_classes: allowed,
        standing_rule_bindings: bindings,
        carve_outs: merged_carve_outs,
    };
    store.store_disclosure_policy(&policy, now)?;
    Ok(policy)
}

fn egress_slug(class: EgressClass) -> &'static str {
    match class {
        EgressClass::Search => "search",
        EgressClass::ForumBrowse => "forum-browse",
        EgressClass::WebFormPost => "web-form-post",
    }
}
fn relationship_slug(relationship: RelationshipKind) -> &'static str {
    match relationship {
        RelationshipKind::Owner => "owner",
        RelationshipKind::Spouse => "spouse",
        RelationshipKind::Family => "family",
        RelationshipKind::Colleague => "colleague",
        RelationshipKind::Client => "client",
        RelationshipKind::Vendor => "vendor",
        RelationshipKind::Unknown => "unknown",
    }
}
fn disclosure_slug(class: DisclosureClass) -> &'static str {
    match class {
        DisclosureClass::Public => "public",
        DisclosureClass::Internal => "internal",
        DisclosureClass::Private => "private",
        DisclosureClass::Sensitive => "sensitive",
    }
}

#[cfg(test)]
mod disclosure_regression_tests;
#[cfg(test)]
mod disclosure_tests;
