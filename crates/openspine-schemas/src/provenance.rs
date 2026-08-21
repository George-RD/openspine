//! Typed provenance-origin vocabulary (spec #220 / #222, canon D-174).
//!
//! A provenance label records *whose* data a classified item is — its
//! typed-identity **origin** — so the deterministic egress gate can later tell
//! one counterparty's datum from another's rather than seeing only a
//! sensitivity class. [`ProvenanceOrigin`] is that origin: a **closed** enum
//! over the owner principal (#197's [`PrincipalId`]), a counterparty
//! ([`IdentityRef`]), or the system. The variant set is closed (never
//! `#[non_exhaustive]`) so every exhaustive `match` in the kernel breaks
//! loudly if an origin kind is ever added — matching the `SkillProvenance`
//! idiom.
//!
//! **Identity is not authority (D-006):** an origin names an identity and
//! nothing else. It carries no capability/route/grant field and grants
//! nothing — enforced structurally by field absence plus
//! `#[serde(deny_unknown_fields)]`, exactly as [`crate::identity::Identity`]
//! is.
//!
//! **Kernel-minted, no worker-set raw string (AD-032 / AD-121, #190).** This
//! type is the *compile-time* half of the provenance hybrid: both non-system
//! origins bind an unforgeable id newtype ([`PrincipalId`] / [`IdentityRef`]),
//! so there is no path that mints an origin from a raw string a worker
//! supplies. The *runtime* half — the label carried in the kernel-owned
//! briefcase/ledger that workers can never mutate (mirroring the kernel-only
//! `Briefcase` mutators) and consulted deterministically at context assembly
//! and egress — lands in the later egress/briefcase tickets (#224–#227), not
//! here.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::ids::{IdentityRef, PrincipalId};

/// The typed identity a classified datum originated from (D-174).
///
/// Closed and internally tagged (`{ "kind": "owner", "principal": "01…" }`),
/// matching the crate's [`crate::task::TaskProvenance`] audit-legible idiom.
/// `#[serde(deny_unknown_fields)]` rejects any injected authority-shaped key,
/// so the wire form can carry origin only (D-006).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProvenanceOrigin {
    /// The owner principal (#197 / AD-146). Reuses the typed [`PrincipalId`]
    /// — never a raw `Ulid` and never an `"owner"` string.
    Owner { principal: PrincipalId },
    /// A counterparty identity, named by its typed [`IdentityRef`].
    Counterparty { identity: IdentityRef },
    /// The kernel/system itself (e.g. proactive, no-principal sends). An
    /// empty *struct* variant, not a unit variant: serde only routes struct
    /// variants through the field visitor that honors
    /// `#[serde(deny_unknown_fields)]`. A unit `System` would silently ignore
    /// injected keys (fail-open), so `System {}` keeps the whole enum
    /// fail-closed against authority-shaped injection.
    System {},
}

impl ProvenanceOrigin {
    /// The kernel/system origin (no principal, no counterparty). The reserved,
    /// non-erasable producing scope in the AD-140 learned-artifact lineage.
    pub const fn system() -> Self {
        ProvenanceOrigin::System {}
    }

    /// The scalar producing scope this origin maps to for AD-140 lineage: the
    /// inner `Ulid` of a counterparty/owner identity, or `Ulid::nil()` — the
    /// reserved system scope (`SYSTEM_SCOPE`) — for the system origin. Kernel
    /// code uses this where a scope value is needed for a scoped blob fetch or
    /// a comparison, keeping the typed origin the single source of truth.
    pub fn producing_scope(&self) -> Ulid {
        match self {
            ProvenanceOrigin::Owner { principal } => principal.as_ulid(),
            ProvenanceOrigin::Counterparty { identity } => identity.as_ulid(),
            ProvenanceOrigin::System {} => Ulid::nil(),
        }
    }
}

#[cfg(test)]
#[path = "provenance_tests.rs"]
mod tests;
