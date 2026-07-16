//! OpenSpine gate/action API.
//!
//! Implements `openspec/changes/implement-gate-action-api/`: the pure
//! `gate()` decision function every effectful action must pass through
//! before a connector dispatcher runs it.

mod gate;

pub use gate::{gate, ActionOrigin, AuditMeta, GateContext, GateOutcome};
