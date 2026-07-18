//! Personality seed artifacts (AD-080/AD-081/AD-082/AD-083).
//!
//! Distinct from [`crate::agent::AgentManifest`] (a bounded worker's
//! manifest, which references a persona by slug) and from
//! [`crate::pack::CapabilityPack`]/[`crate::policy::Policy`] (authority
//! sources — action lists and constraints only, PRD §9). A `PersonaElement`
//! carries no authority; it is free-text behavioral guidance shipped as a
//! pre-populated, learnable overlay artifact — never a kernel-baked fixture
//! (AD-080). Negative constraints (AD-081/AD-083 anti-patterns) are
//! deliberately absent from this schema: AD-054 keeps them as eval probes,
//! never as prompt text baked into an artifact.

use serde::{Deserialize, Serialize};

use crate::artifact::Lifecycle;
use crate::ids::ArtifactId;

/// One pre-populated Donna×Leo personality element (AD-080's eight) or the
/// AD-082 digest/brief format default — each ships as its own overlay
/// artifact so it can converge independently as the owner corrects it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonaElement {
    pub id: ArtifactId,
    pub schema_version: u32,
    #[serde(default = "crate::artifact::default_version")]
    pub version: u32,
    pub lifecycle_state: Lifecycle,
    /// Positive-framed behavioral guidance (AD-054: corrections rewrite
    /// instructions, they never append prohibitions) — no "don't" list.
    pub guidance: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persona_element_round_trips_through_serde() {
        let element = PersonaElement {
            id: "anticipatory_provisioning".to_string(),
            schema_version: 1,
            version: 1,
            lifecycle_state: Lifecycle::Active,
            guidance: "Prepare what the owner will need next with a stated reason.".to_string(),
        };
        let json = serde_json::to_string(&element).unwrap();
        let back: PersonaElement = serde_json::from_str(&json).unwrap();
        assert_eq!(element, back);
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let yaml = "id: x\nschema_version: 1\nversion: 1\nlifecycle_state: active\nguidance: g\nbogus: true\n";
        let result: Result<PersonaElement, _> = serde_yaml::from_str(yaml);
        assert!(result.is_err());
    }
}
