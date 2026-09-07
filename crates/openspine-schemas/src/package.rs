//! Assistant package declarations (#273/#274).
//!
//! A declaration is descriptive data, not an authority or activation decision.
//! Inspection must separately resolve the captured artifact inventory and active
//! entry agent through the existing loader. Parsing does not verify a publisher.

use serde::{Deserialize, Serialize};

use crate::ids::ArtifactId;

/// Schema-v1 declaration shipped as `package.yaml`. Package revisions are
/// positive integers, not runtime semantic versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageDeclaration {
    pub schema_version: u32,
    pub id: String,
    pub version: u32,
    /// Descriptive release label; never an artifact lifecycle transition.
    pub lifecycle_state: String,
    pub display_name: String,
    pub description: String,
    pub entry_agent: ArtifactId,
    pub artifacts: PackageArtifacts,
    pub identity: PackageIdentity,
    pub memory: PackageMemory,
    pub installation: PackageInstallation,
    pub security_invariants: Vec<String>,
}

/// The seven supported declaration families. A logical ID may appear in
/// different families; versions and source collisions belong to the loader.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageArtifacts {
    pub agents: Vec<ArtifactId>,
    pub routes: Vec<ArtifactId>,
    pub workflows: Vec<ArtifactId>,
    pub packs: Vec<ArtifactId>,
    pub templates: Vec<ArtifactId>,
    pub policies: Vec<ArtifactId>,
    pub golden_sets: Vec<ArtifactId>,
}

/// Identity descriptions are not privileged runtime inputs or path directives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageIdentity {
    pub persona: String,
    pub identity_document: String,
}

/// Product-defined memory vocabulary, not grants or disclosure permissions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageMemory {
    pub model: String,
    pub description: String,
    pub readable_classes: Vec<String>,
    pub readable_scopes: Vec<String>,
    pub denied_classes: Vec<String>,
}

/// Installation documentation only. These strings never select destinations,
/// configuration keys to mutate, or commands to execute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageInstallation {
    pub current_source: String,
    pub config_key: String,
    pub planned_cli: String,
}

#[cfg(test)]
#[path = "package_tests.rs"]
mod tests;
