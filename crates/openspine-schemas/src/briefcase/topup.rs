use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use super::{BriefcaseSection, RelationshipTier, SectionKind};

/// The filtered, owned projection of a [`Briefcase`](super::Briefcase) a worker receives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BriefcaseView {
    pub worker_id: Ulid,
    pub sections: Vec<BriefcaseSection>,
}

/// Kernel standing top-up policy keyed by relationship/task/section.
#[derive(Debug, Clone, Default)]
pub struct TopUpPolicy {
    max_depth: BTreeMap<(RelationshipTier, super::TaskClass, SectionKind), u8>,
    max_total_sections: BTreeMap<(RelationshipTier, super::TaskClass), u8>,
}

impl TopUpPolicy {
    pub fn new(
        rules: impl IntoIterator<Item = ((RelationshipTier, super::TaskClass, SectionKind), u8)>,
    ) -> Self {
        Self {
            max_depth: rules.into_iter().collect(),
            max_total_sections: BTreeMap::new(),
        }
    }

    pub fn with_max_total_sections(
        mut self,
        rules: impl IntoIterator<Item = ((RelationshipTier, super::TaskClass), u8)>,
    ) -> Self {
        self.max_total_sections = rules.into_iter().collect();
        self
    }

    pub fn max_depth_for(
        &self,
        tier: RelationshipTier,
        class: super::TaskClass,
        kind: SectionKind,
    ) -> Option<u8> {
        self.max_depth.get(&(tier, class, kind)).copied()
    }

    pub fn max_total_sections_for(
        &self,
        tier: RelationshipTier,
        class: super::TaskClass,
    ) -> Option<u8> {
        self.max_total_sections.get(&(tier, class)).copied()
    }
}

/// Worker-authored request for additional context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TopUpRequest {
    pub request_id: Ulid,
    pub section_key: String,
    pub kind: SectionKind,
    pub requested_depth: u8,
    pub justification: String,
}

impl TopUpRequest {
    pub const MAX_SECTION_KEY_BYTES: usize = 128;

    pub fn for_persistence(&self) -> Self {
        let mut cloned = self.clone();
        cloned.section_key = format!(
            "key:{}",
            crate::digest::digest_of(&serde_json::Value::String(self.section_key.clone()))
        );
        cloned.justification =
            crate::digest::digest_of(&serde_json::Value::String(self.justification.clone()))
                .as_str()
                .to_owned();
        cloned
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum TopUpOutcome {
    Allowed,
    Denied { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TopUpDecision {
    pub request: TopUpRequest,
    pub outcome: TopUpOutcome,
    /// Content digest resolved by the kernel before authorization.
    /// Allowed decisions without this binding cannot be applied.
    #[serde(default)]
    pub source_digest: Option<crate::digest::Digest>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnedOutput {
    pub outcome: String,
    #[serde(default)]
    pub offered_slots: Vec<String>,
    #[serde(default)]
    pub requests: Vec<TopUpRequest>,
}
