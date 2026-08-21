//! Per-connector preparation: derive the classified provenance the
//! connector-agnostic disclosure core evaluates.
//!
//! Web-search preparation keeps its existing shape — a free-text query
//! generalized by redacting sensitive terms before transport. Messaging-send
//! preparation (email/Telegram/future WhatsApp) derives provenance from the
//! classified briefcase sections the already-composed content cites and never
//! generalizes the message body itself (the recipient is meant to read it).

use super::*;

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
            // Carry the kernel-derived origin verbatim from the section. Egress
            // coverage still keys only on `disclosure_class`; the origin rides
            // alongside for the origin-vs-recipient closure landing in a later
            // egress ticket (#225–#227) and is part of the minted binding.
            origin: section.origin.clone(),
        });
    }
    Ok(DisclosureProvenance { items })
}

/// Web-search preparation: generalize the raw free-text query by redacting the
/// sensitive terms found in the grant's private/sensitive sections, bind it to
/// the request, and persist a one-use prepared-query token.
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

/// Messaging-send preparation (email/Telegram/future WhatsApp) for content
/// addressed to one verified recipient. Unlike web-search preparation, the
/// already-composed message body is NOT generalized — the recipient is meant
/// to read it — so `sensitive_terms` is empty and the disclosure core's
/// decision is driven purely by the kernel-derived provenance of the sections
/// the content cites, never by an LLM's judgment of the text.
///
/// `recipient` records the single-verified-recipient intent; this function
/// does not resolve or deliver to any recipient and rates no action. It is
/// called from the `email.send` dispatch path (#206), which resolves the
/// egress class from the catalog before invoking it.
pub(crate) fn prepare_messaging_disclosure(
    action_id: ActionId,
    relationship: RelationshipKind,
    recipient: &str,
    composed_content: String,
    sections: &[BriefcaseSection],
) -> Result<DisclosureRequest, DisclosureError> {
    // The recipient binding is validated by dispatch (selection token), not
    // here — this prefactor only records that messaging egress targets exactly
    // one verified counterparty.
    let _ = recipient;
    let provenance = provenance_from_sections(sections)?;
    Ok(DisclosureRequest {
        raw_query: composed_content,
        sensitive_terms: BTreeSet::new(),
        action_id,
        relationship,
        provenance,
    })
}
