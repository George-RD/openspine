//! OpenSpine authority composition.
//!
//! Implements `openspec/changes/implement-authority-composition/`: pure,
//! no-I/O functions `resolve_route` and `compose_authority` that merge
//! route/workflow/agent/pack/policy inputs into a `TaskGrant` or a denial.

mod compose;
mod route;

pub use compose::{compose_authority, AuthorityInput, AuthorityOutcome};
pub use route::resolve_route;
