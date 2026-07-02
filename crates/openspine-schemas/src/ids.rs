//! Identifier conventions.
//!
//! OpenSpine has two distinct id shapes (PRD §4–§12): declarative artifacts
//! (routes, agents, workflows, capability packs, policies) use a stable,
//! human-readable slug chosen at authoring time (e.g. `main_assistant_agent`);
//! runtime instances (events, identities, task grants, approvals, selection
//! tokens, model requests, audit events) use a [`ulid::Ulid`] minted at
//! creation time. `ArtifactId` names the former; the latter is `ulid::Ulid`
//! directly.

/// A stable, human-authored identifier for a declarative artifact.
pub type ArtifactId = String;
