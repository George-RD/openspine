//! `gate()` — the single mediation point every effectful action passes
//! through (design.md, PRD §8.3, spec.md).
//!
//! Pure decision logic: no storage, no I/O. Approval and selection-token
//! *lookups* are supplied by the caller (the kernel, Step 4/5) through
//! [`GateContext`] so this crate never touches SQLite directly.

use jiff::Timestamp;
use ulid::Ulid;

use openspine_schemas::action::{
    ActionCatalog, ActionId, ActionRequest, DenialReason, GateDecision,
};
use openspine_schemas::approval::{ApprovalDecision, ApprovalRecord};
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::Digest;
use openspine_schemas::event::TargetRef;
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::selection::SelectionToken;

/// Everything a caller must be able to look up for `gate()` to resolve one
/// [`ActionRequest`] without doing storage I/O itself.
pub trait GateContext {
    /// The approval decision recorded against this exact `action_request_id`,
    /// if the owner has already decided one way or another (approved,
    /// rejected, or edited). `None` means "never asked" — the only path
    /// that leads back to [`GateDecision::ApprovalRequired`]. Once a
    /// decision exists, a request whose payload/target digest no longer
    /// matches it is denied outright (D-011), never re-asked.
    fn approval_for_request(&self, action_request_id: Ulid) -> Option<ApprovalRecord>;

    /// Look up a selection token by id. Not called by this change's own
    /// `gate()` body — declared now so the trait boundary is stable before
    /// Step 5 wires selection-token validation into connector dispatch.
    fn find_selection_token(&self, id: Ulid) -> Option<SelectionToken>;
}

/// Audit-sufficient metadata for one gate decision (spec.md "Gate decisions
/// MUST be auditable"). Private payloads are represented by refs/digests
/// only — [`ArtifactRef`] carries a digest and lifecycle state, never
/// plaintext (PRD §18); `target_ref`/`target_digest` mirror whatever the
/// request carried, so a denial can be traced back to exactly what was
/// being acted on without ever recording raw content.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditMeta {
    pub action: ActionId,
    pub task_grant_id: Ulid,
    pub target_ref: Option<TargetRef>,
    pub target_digest: Option<Digest>,
    pub payload_refs: Vec<ArtifactRef>,
}

/// The full outcome of mediating one [`ActionRequest`]: the decision plus
/// enough metadata for the caller to persist an audit event. `gate()` never
/// writes the audit event itself (no I/O) — it returns what the write needs.
#[derive(Debug, Clone, PartialEq)]
pub struct GateOutcome {
    pub decision: GateDecision,
    pub audit: AuditMeta,
}

/// Mediate one action request against its task grant (PRD §8.3).
///
/// Precedence: explicit deny > approval-required > allow > unspecified deny.
/// A grant that has already expired is denied before any list is consulted
/// — an expired grant authorizes nothing, no matter what its lists say.
pub fn gate(
    grant: &TaskGrant,
    req: &ActionRequest,
    ctx: &dyn GateContext,
    catalog: &ActionCatalog,
    now: Timestamp,
) -> GateOutcome {
    GateOutcome {
        decision: resolve(grant, req, ctx, catalog, now),
        audit: AuditMeta {
            action: req.action.clone(),
            task_grant_id: grant.id,
            target_ref: req.target_ref.clone(),
            target_digest: req.target_digest.clone(),
            payload_refs: req.payload_ref.iter().cloned().collect(),
        },
    }
}

fn resolve(
    grant: &TaskGrant,
    req: &ActionRequest,
    ctx: &dyn GateContext,
    catalog: &ActionCatalog,
    now: Timestamp,
) -> GateDecision {
    if grant.is_expired(now) {
        return GateDecision::Deny {
            reason: DenialReason::GrantExpired,
        };
    }

    // D-053: an action id outside the canonical catalog is not a "known but
    // ungranted" action — it is outside the action universe entirely. The
    // check precedes the grant lists on purpose: composition fail-fast keeps
    // unknown ids out of NEW grants, but a grant persisted before the catalog
    // existed could still carry one, and the gate is the last line of
    // defense — such an id must never resolve to Allow (or any list-derived
    // reason) from stale grant state.
    if !catalog.contains(&req.action) {
        return GateDecision::Deny {
            reason: DenialReason::UnknownAction,
        };
    }

    if grant.denied_actions.contains(&req.action) {
        return GateDecision::Deny {
            reason: DenialReason::ExplicitDeny,
        };
    }

    if grant.approval_required_actions.contains(&req.action) {
        return resolve_approval_required(req, ctx, now);
    }

    if grant.allowed_actions.contains(&req.action) {
        return GateDecision::Allow;
    }

    GateDecision::Deny {
        reason: DenialReason::NotGranted,
    }
}

fn resolve_approval_required(
    req: &ActionRequest,
    ctx: &dyn GateContext,
    now: Timestamp,
) -> GateDecision {
    let Some(approval) = ctx.approval_for_request(req.id) else {
        return GateDecision::ApprovalRequired {
            approval_type: req.action.as_str().to_string(),
        };
    };

    let payload_digest: Option<&Digest> = req.payload_ref.as_ref().map(|r| &r.digest);
    let target_digest: Option<&Digest> = req.target_digest.as_ref();

    let currently_matches = match (payload_digest, target_digest) {
        (Some(pd), Some(td)) => approval.matches(pd, td, now),
        // Nothing to bind the approval to — it can never authorize this
        // request, digest-bound or not.
        _ => false,
    };

    if currently_matches {
        return GateDecision::Allow;
    }

    // An approval decision exists for this exact action request but does
    // not currently authorize it. This is a denial, never a re-ask: an
    // agent that mutates a payload/target after approval must not be able
    // to walk gate() back into ApprovalRequired and try again unreviewed.
    let reason = if now >= approval.expires_at {
        DenialReason::ApprovalExpired
    } else if approval.decision != ApprovalDecision::Approved {
        DenialReason::ApprovalMissing
    } else {
        DenialReason::ApprovalDigestMismatch
    };
    GateDecision::Deny { reason }
}

#[cfg(test)]
mod tests;
