//! Relationship-scoped disclosure egress gate.
//!
//! Two layers, split by the seam #193 (provenance labels) will extend:
//! - [`core`] — the connector-agnostic deterministic enforcement core.
//! - [`preparation`] — per-connector derivation of the provenance the core
//!   evaluates (web-search query generalization and messaging-content shapes).
//!
//! The shared request/response types live here so both submodules and the
//! test suites (which `use super::*`) see one surface; the `use` block below
//! is the shared import set both submodules pull in via `use super::*`.

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

mod core;
mod preparation;

pub(crate) use self::core::*;
pub(crate) use self::preparation::*;

#[cfg(test)]
mod disclosure_messaging_tests;
#[cfg(test)]
mod disclosure_regression_tests;
#[cfg(test)]
mod disclosure_tests;
