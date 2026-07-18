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
use openspine_schemas::egress::EgressClass;
use openspine_schemas::event::TargetRef;
use openspine_schemas::grant::TaskGrant;
use openspine_schemas::selection::{SelectionToken, SelectionTokenType};

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

    /// Kernel-owned HMAC verification key. `None` fails closed — every
    /// grant presented to `gate()` MUST carry a verified chain tip.
    fn grant_hmac_key(&self) -> Option<Vec<u8>> {
        None
    }
}

/// Where an action request originates from (D-055.3).
///
/// The gate treats kernel-origin effects as trusted *by default* for the
/// catalog's enumerated trusted-origin set: an `owner.notify`-style effect
/// that the kernel itself emits (never a shell) is allowed without a granting
/// decision. A kernel-origin request for any action OUTSIDE that set is
/// denied — the kernel may not reach for arbitrary actions without being
/// explicitly enumerated as trusted. Shell-origin requests (the default for
/// everything a connector dispatcher runs) follow the normal granting
/// decision path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionOrigin {
    /// A request emitted by the kernel's own pipeline (e.g. notify-owner),
    /// trusted only for the catalog's `kernel_origin_actions` set.
    Kernel,
    /// A request submitted by a shell through a connector dispatcher. The
    /// default; follows the normal grant-listing decision path.
    Shell,
}

/// Trusted resolver for the egress class of a rated endpoint (AD-060).
///
/// The gate queries this — never the request — so omission or spoofing by
/// the shell cannot make the egress-class check fail open. The kernel's
/// connector registry implements this; tests that do not exercise egress
/// pass [`NoEgress`].
pub trait EgressClassifier {
    /// Return the egress class for a rated endpoint, or `None` if the
    /// action is not a rated egress endpoint.
    fn classify(&self, action: &ActionId) -> Option<EgressClass>;
}

/// No-op classifier for contexts without rated egress endpoints.
/// Existing gate tests and non-egress call paths use this; the check
/// becomes a no-op for unrated actions.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoEgress;

impl EgressClassifier for NoEgress {
    fn classify(&self, _action: &ActionId) -> Option<EgressClass> {
        None
    }
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
    pub origin: ActionOrigin,
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
    origin: ActionOrigin,
    ctx: &dyn GateContext,
    catalog: &ActionCatalog,
    _egress: &dyn EgressClassifier,
    now: Timestamp,
) -> GateOutcome {
    let mut decision = resolve(grant, req, origin, ctx, catalog, _egress, now);
    if grant.mode == openspine_schemas::grant::GrantMode::Shadow
        && matches!(
            decision,
            GateDecision::Allow | GateDecision::ApprovalRequired { .. }
        )
    {
        decision = GateDecision::EffectSuppressed;
    }
    GateOutcome {
        decision,
        audit: AuditMeta {
            action: req.action.clone(),
            task_grant_id: grant.id,
            origin,
            target_ref: req.target_ref.clone(),
            target_digest: req.target_digest.clone(),
            payload_refs: req.payload_ref.iter().cloned().collect(),
        },
    }
}

fn chain_valid(grant: &TaskGrant, ctx: &dyn GateContext) -> bool {
    // Every TaskGrant carries an authenticated chain. The generic verifier
    // rejects gate-specific caveats, but this upgraded gate explicitly
    // handles both allowlist caveats below and may declare them supported.
    if grant.caveat_mac.is_empty() {
        return false;
    }
    let Some(key) = ctx.grant_hmac_key() else {
        return false;
    };
    grant.verify_mac(&key)
        && openspine_schemas::grant_chain::chain_structurally_valid(grant)
        && !openspine_schemas::grant_chain::has_unsupported_caveats_except(
            grant,
            &[
                openspine_schemas::grant_chain::SupportedCaveatKind::OutputChannelAllowlist,
                openspine_schemas::grant_chain::SupportedCaveatKind::EgressClassAllowlist,
            ],
        )
}

fn resolve(
    grant: &TaskGrant,
    req: &ActionRequest,
    origin: ActionOrigin,
    ctx: &dyn GateContext,
    catalog: &ActionCatalog,
    _egress: &dyn EgressClassifier,
    now: Timestamp,
) -> GateDecision {
    // D-004: authority/MAC failures classify as CaveatWidening even when the
    // grant is also expired — chain integrity is checked before expiry.
    if !chain_valid(grant, ctx) || !openspine_schemas::grant_chain::bindings_valid(grant) {
        return GateDecision::Deny {
            reason: DenialReason::CaveatWidening,
        };
    }
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

    // Explicit shell denies retain precedence over every later policy
    // constraint, including rated-egress coverage. Kernel-origin requests
    // use the trusted-origin path below instead of shell grant lists.
    if origin == ActionOrigin::Shell && grant.denied_actions.contains(&req.action) {
        return GateDecision::Deny {
            reason: DenialReason::ExplicitDeny,
        };
    }

    // AD-060 / AD-035: egress metadata is catalog-owned (blocker 1). The gate
    // reads the mandatory per-action declaration and fails closed when it is
    // absent; connector metadata is never consulted. Egress-class enforcement
    // runs before the kernel-origin early return on purpose: endpoint typing
    // is a structural property of the action, not part of the grant-list
    // "granting decision" kernel-origin is defined to skip. The gate resolves
    // the class from the trusted catalog (never from the request), and denies
    // if the grant does not cover it. A pack granted search-class egress
    // cannot submit a web form, whether the request is shell-origin or
    // kernel-origin.
    if let Some(decl) = catalog.egress_decl_for(&req.action) {
        if let Some(class) = decl.egress_class {
            if !openspine_schemas::grant_chain::effectively_allows_egress_class(grant, &class) {
                return GateDecision::Deny {
                    reason: DenialReason::EgressClassNotGranted,
                };
            }
        }
        if let Some(channels) = &decl.output_channels {
            if channels.is_empty()
                || channels.iter().any(|channel| {
                    !openspine_schemas::grant_chain::effectively_allows_output_channel(
                        grant, channel,
                    )
                })
            {
                return GateDecision::Deny {
                    reason: DenialReason::OutputChannelNotGranted,
                };
            }
        }
    } else if catalog.contains(&req.action) {
        // Registered action with no mandatory egress declaration: catalog
        // integrity violation → fail closed rather than treat as unrated.
        return GateDecision::Deny {
            reason: DenialReason::CaveatWidening,
        };
    }

    // AD-036: a `BoundParameter` caveat locks a request parameter name to a
    // value. The gate enforces it against the ACTUAL request params (blocker
    // 2); a bound name absent from the request or carrying a different value
    // is a caveat violation, fail closed.
    for caveat in openspine_schemas::grant_chain::flattened_caveats(&grant.chain) {
        if let openspine_schemas::grant_chain::Caveat::BoundParameter { name, value } = caveat {
            match req.params.get(name) {
                Some(actual) if actual == value => {}
                _ => {
                    return GateDecision::Deny {
                        reason: DenialReason::CaveatWidening,
                    };
                }
            }
        }
    }

    // D-055.3: kernel-origin effects are trusted only for the catalog's
    // enumerated trusted-origin set. A kernel request for any action outside
    // that set is denied outright — the kernel may not reach for arbitrary
    // actions without being explicitly enumerated as trusted. Shell-origin
    // requests fall through to the normal granting decision.
    if origin == ActionOrigin::Kernel {
        if catalog.is_kernel_origin(&req.action) {
            return GateDecision::Allow;
        }
        return GateDecision::Deny {
            reason: DenialReason::KernelOriginNotTrusted,
        };
    }

    // D-055.1: if the catalog requires a selection token for this action,
    // the request must carry a valid, grant-bound, unexpired selection token
    // of the correct type before any listing decision is consulted.
    if let Some(expected_type) = catalog.requires_selection_token(&req.action) {
        if let Some(reason) = validate_selection_token(grant, req, expected_type, ctx, now) {
            return GateDecision::Deny { reason };
        }
    }

    if grant.effectively_approval_required(&req.action) {
        return resolve_approval_required(req, ctx, now);
    }

    if grant.effectively_allows(&req.action) {
        return GateDecision::Allow;
    }

    GateDecision::Deny {
        reason: DenialReason::NotGranted,
    }
}

/// D-055.1: validate the selection token carried by a request for a
/// `token_requiring` action. Returns `Some(reason)` to deny, or `None` to
/// let the token check pass.
fn validate_selection_token(
    grant: &TaskGrant,
    req: &ActionRequest,
    expected_type: &SelectionTokenType,
    ctx: &dyn GateContext,
    now: Timestamp,
) -> Option<DenialReason> {
    let token_id = match req.selection_token_id {
        Some(id) => id,
        None => return Some(DenialReason::SelectionTokenInvalid),
    };

    // Bound to this grant: a token minted for a different grant must not
    // authorize an action under this one (D-055.1).
    if !grant.selection_tokens.contains(&token_id) {
        return Some(DenialReason::SelectionTokenInvalid);
    }

    let token = match ctx.find_selection_token(token_id) {
        Some(t) => t,
        None => return Some(DenialReason::SelectionTokenInvalid),
    };

    if &token.token_type != expected_type {
        return Some(DenialReason::SelectionTokenInvalid);
    }

    if token.expires_at <= now {
        return Some(DenialReason::SelectionTokenInvalid);
    }

    None
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
mod plan_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod token_tests;
