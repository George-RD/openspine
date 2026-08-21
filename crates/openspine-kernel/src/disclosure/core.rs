//! Connector-agnostic deterministic disclosure core.
//!
//! Given a request's kernel-derived classified provenance, its relationship,
//! and its (catalog-resolved) egress class, evaluate policy coverage via the
//! pure `check_egress`, reserve/cancel the scoped D-107 standing-rule budget
//! all-or-nothing, and on an uncovered combination raise the existing
//! `OwnerQuestion` escalation. This core is preparation-agnostic: it never
//! looks at how the provenance was derived (web-search query generalization or
//! messaging-content section citation) and never inspects query text.

use super::*;

pub(crate) fn action_for_scope(key: DisclosurePolicyKey, class: EgressClass) -> ActionId {
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
    // Resolve the typed recipient this grant's task is bound to reach from the
    // kernel-owned briefcase (mirrors the `is_counterparty_erased` precedent).
    // An unresolved counterparty — or no briefcase — is fail-closed: the origin
    // closure then matches nothing. Resolved uniformly here so every dispatch
    // origin (worker-requested and kernel-origin/proactive) inherits the
    // identical check through this one chokepoint — no second ungated path.
    let recipient = match state
        .store
        .find_briefcase(grant.id)
        .map_err(DisclosureError::Store)?
    {
        Some(briefcase) => match briefcase.task_shape.counterparty {
            CounterpartyRef::Bound { identity_id, .. } => RecipientIdentity::Counterparty {
                identity: IdentityRef::from(identity_id),
            },
            CounterpartyRef::Unresolved { .. } => RecipientIdentity::Unresolved,
        },
        None => RecipientIdentity::Unresolved,
    };
    // Pre-resolve the grant-authorized origin set (D-174 widening). The pure
    // core stays grant-free and only tests membership. The v1 owner grant
    // carries no `ProvenanceLabelAllowlist` caveat, so every present origin is
    // authorized and the closure is dormant; a worker sub-grant's empty caveat
    // authorizes none, closing the origin set strictly.
    let mut authorized_origins: Vec<ProvenanceOrigin> = Vec::new();
    for item in &request.provenance.items {
        if let Some(origin) = &item.origin {
            if !authorized_origins.contains(origin)
                && openspine_schemas::grant_chain::effectively_allows_provenance_label(
                    grant, origin,
                )
            {
                authorized_origins.push(origin.clone());
            }
        }
    }
    let query = OutboundQuery::from_private_context(
        &request.raw_query,
        &request.sensitive_terms,
        egress_class,
        request.provenance,
        recipient,
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
    match check_egress(request.relationship, query, &policies, &authorized_origins) {
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
        openspine_schemas::disclosure_policy::DisclosureGateDecision::CrossIdentityBlock {
            origin,
            recipient,
            disclosure_class,
            egress_class: blocked_egress,
        } => {
            cancel_reservations(&state.store, &reservations);
            // Auditor (D-174): the block is reconstructible from typed origin +
            // sensitivity + recipient + egress class + relationship. Detail is
            // kernel-side only; the worker sees the generic denial mapped in
            // api/actions.rs, never these internals.
            let detail = format!(
                "cross-identity egress blocked: origin={origin:?} recipient={recipient:?} class={disclosure_class:?} egress={blocked_egress:?} relationship={:?}",
                request.relationship
            );
            state
                .store
                .append_audit(
                    "disclosure.cross_identity_blocked",
                    Some(&request.action_id),
                    None,
                    Some(&detail),
                    Some(grant.id),
                    &[],
                    &[],
                )
                .map_err(DisclosureError::Store)?;
            // Inform the owner (AD-133) but mint no disclosure pending question:
            // unlike a coverage block, a cross-identity block is not resolved by
            // a relationship/class "/disclosure allow" — widening an origin is a
            // grant-caveat decision, never a runtime owner answer.
            let event = EscalationEvent::owner_question(
                grant.id,
                format!(
                    "Blocked outbound {disclosure_class:?} data whose origin is outside the recipient's identity closure ({blocked_egress:?})."
                ),
                grant.thread_id.clone(),
                now,
            );
            route_escalation(state, grant, &event)
                .await
                .map_err(DisclosureError::Store)?;
            Err(DisclosureError::CrossIdentityBlocked)
        }
    }
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
        reviewed_scope: None,
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
        EgressClass::DirectMessage => "direct-message",
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
