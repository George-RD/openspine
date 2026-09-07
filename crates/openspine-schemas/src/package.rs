//! Assistant package declarations (#273/#274).
//!
//! A declaration is descriptive data, not an authority or activation decision.
//! Inspection must separately resolve the captured artifact inventory and active
//! entry agent through the existing loader. Parsing does not verify a publisher.
//!
//! Deserialize bounded source bytes directly into `PackageDeclaration`, not
//! through an intermediate map that can discard duplicate keys. These checks
//! validate the declaration only, not its source tree or runtime compatibility.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::ids::ArtifactId;

/// The only assistant-package declaration schema supported by this build.
pub const CURRENT_PACKAGE_SCHEMA_VERSION: u32 = 1;

/// Schema-v1 declaration shipped as `package.yaml`. Package revisions are
/// positive integers, not runtime semantic versions. Field guards apply during
/// deserialization; this public data object is not a validated-package token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageDeclaration {
    #[serde(deserialize_with = "deserialize_schema_version")]
    pub schema_version: u32,
    #[serde(deserialize_with = "deserialize_package_id")]
    pub id: String,
    #[serde(deserialize_with = "deserialize_revision")]
    pub version: u32,
    /// Descriptive release label; never an artifact lifecycle transition.
    pub lifecycle_state: String,
    pub display_name: String,
    pub description: String,
    #[serde(deserialize_with = "deserialize_string")]
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
    #[serde(deserialize_with = "deserialize_artifact_ids")]
    pub agents: Vec<ArtifactId>,
    #[serde(deserialize_with = "deserialize_artifact_ids")]
    pub routes: Vec<ArtifactId>,
    #[serde(deserialize_with = "deserialize_artifact_ids")]
    pub workflows: Vec<ArtifactId>,
    #[serde(deserialize_with = "deserialize_artifact_ids")]
    pub packs: Vec<ArtifactId>,
    #[serde(deserialize_with = "deserialize_artifact_ids")]
    pub templates: Vec<ArtifactId>,
    #[serde(deserialize_with = "deserialize_artifact_ids")]
    pub policies: Vec<ArtifactId>,
    #[serde(deserialize_with = "deserialize_artifact_ids")]
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

fn deserialize_schema_version<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let version = u32::deserialize(deserializer)?;
    if version != CURRENT_PACKAGE_SCHEMA_VERSION {
        return Err(serde::de::Error::custom(
            "unsupported package schema_version",
        ));
    }
    Ok(version)
}

fn deserialize_revision<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let version = u32::deserialize(deserializer)?;
    if version == 0 {
        return Err(serde::de::Error::custom("package version must be positive"));
    }
    Ok(version)
}

fn deserialize_package_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let id = deserialize_string(deserializer)?;
    let valid = (1..=64).contains(&id.len())
        && id.as_bytes()[0].is_ascii_lowercase()
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        });
    if !valid {
        return Err(serde::de::Error::custom(
            "package id must match [a-z][a-z0-9_-]{0,63}",
        ));
    }
    Ok(id)
}

// Serde YAML's ordinary String deserializer can coerce scalars such as null to
// text. IDs must retain their actual string type, not acquire an implicit name.
fn deserialize_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct StringVisitor;

    impl serde::de::Visitor<'_> for StringVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a string")
        }

        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<String, E> {
            Ok(value.to_owned())
        }

        fn visit_string<E: serde::de::Error>(self, value: String) -> Result<String, E> {
            Ok(value)
        }
    }

    deserializer.deserialize_any(StringVisitor)
}

#[derive(Deserialize)]
#[serde(transparent)]
struct DeclaredArtifactId(#[serde(deserialize_with = "deserialize_string")] ArtifactId);

fn deserialize_artifact_ids<'de, D>(deserializer: D) -> Result<Vec<ArtifactId>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let entries = Vec::<DeclaredArtifactId>::deserialize(deserializer)?;
    let ids: Vec<_> = entries.into_iter().map(|entry| entry.0).collect();
    if ids.iter().collect::<BTreeSet<_>>().len() != ids.len() {
        return Err(serde::de::Error::custom(
            "duplicate logical id in package artifact family",
        ));
    }
    Ok(ids)
}

#[cfg(test)]
#[path = "package_tests.rs"]
mod tests;
