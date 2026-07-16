//! Principal records (AD-146). A Principal is a first-class record that
//! authority composition keys off of. v1 enforces exactly one Principal
//! (the owner). A Principal is NOT authority (D-006): it carries no
//! capability/route/grant fields — it is the identity-shaped key the
//! kernel composes a grant FOR, never a grant itself. Counterparties
//! (even richly bound ones) are NOT principals in v1.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Principal {
    pub id: Ulid,
    /// The identity record this principal is. For the owner, the
    /// bootstrapped owner identity.
    pub identity_id: Ulid,
    /// v1: exactly one principal has is_owner == true (AD-146).
    pub is_owner: bool,
    pub schema_version: u32,
}
