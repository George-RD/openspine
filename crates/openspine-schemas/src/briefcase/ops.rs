//! [`Briefcase`] operations: canonical serialization, filtered worker views,
//! returned-output export, and top-up evaluation/application.

use std::collections::BTreeMap;

use serde_json::Value;

use super::{
    canonical_json, digest_of, Briefcase, BriefcaseError, BriefcaseSection, BriefcaseView,
    SectionKind, SourceSlice, TopUpDecision, TopUpOutcome, TopUpPolicy, TopUpRequest,
    VisibilityClass, WorkerVisibility,
};

impl Briefcase {
    /// The canonical-JSON pre-image bytes of this pack (D-028's canonical
    /// digesting convention, reused here for the determinism test rather
    /// than a fresh ad hoc comparison): recursive key-sort, no
    /// insignificant whitespace. Two packs are byte-identical here iff
    /// every field — including `source_snapshot_id` — matches.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let value = serde_json::to_value(self).expect("Briefcase always serializes");
        canonical_json(&value).into_bytes()
    }

    /// The filtered, read-only projection a worker actually receives
    /// (AD-121's fog-of-war visibility schema). `KernelBound` sections are
    /// NEVER emitted here, regardless of what `visibility.allowed`
    /// contains — that exclusion is structural, not merely policy, because
    /// a worker never gets anything but this projection (module-level
    /// confused-deputy note).
    pub fn view_for(&self, visibility: &WorkerVisibility) -> BriefcaseView {
        let sections = self
            .sections
            .iter()
            .filter(|s| s.visibility != VisibilityClass::KernelBound)
            .filter(|s| visibility.allowed.contains(&s.visibility))
            .cloned()
            .collect();
        BriefcaseView {
            worker_id: visibility.worker_id,
            sections,
        }
    }

    /// The ONLY sanctioned way to pull section content into an outbound
    /// (worker→master) payload: every requested key must already be
    /// classified `ReturnedOutput`, or this refuses the whole export. A
    /// `WorkerScratch` (or `KernelBound`) key can never cross this boundary
    /// by being asked for, even by kernel-side code — the guard is on the
    /// read, not on trusting the caller's intent.
    pub fn export_returned_output(
        &self,
        keys: &[String],
    ) -> Result<BTreeMap<String, Value>, BriefcaseError> {
        let mut out = BTreeMap::new();
        for key in keys {
            let section = self
                .sections
                .iter()
                .find(|s| &s.key == key)
                .ok_or_else(|| BriefcaseError::SectionNotFound(key.clone()))?;
            if section.visibility != VisibilityClass::ReturnedOutput {
                return Err(BriefcaseError::VisibilityViolation {
                    key: key.clone(),
                    actual: section.visibility,
                });
            }
            out.insert(key.clone(), section.payload.clone());
        }
        Ok(out)
    }

    /// AD-031/AD-032: mediate one worker top-up request through the
    /// standing depth policy — never the generic action gate (see
    /// `openspine-kernel::briefcase`'s module doc for why). Pure decision;
    /// does not mutate `self`.
    pub fn evaluate_top_up(&self, request: &TopUpRequest, policy: &TopUpPolicy) -> TopUpDecision {
        let max_depth = policy.max_depth_for(self.tier, self.class, request.kind);
        let ceiling = policy
            .max_total_sections_for(self.tier, self.class)
            .unwrap_or(self.depth);

        let key = format!("{:?}:{}", request.kind, request.section_key).to_lowercase();
        let is_new_key = !self.sections.iter().any(|section| section.key == key);
        let content_count = self
            .sections
            .iter()
            .filter(|section| matches!(section.kind, SectionKind::Preference | SectionKind::Skill))
            .count();

        let outcome = match max_depth {
            Some(max_depth) if request.requested_depth <= max_depth => {
                if is_new_key && content_count >= ceiling as usize {
                    TopUpOutcome::Denied {
                        reason: format!("top-up would exceed aggregate depth budget of {ceiling}"),
                    }
                } else {
                    TopUpOutcome::Allowed
                }
            }
            Some(max_depth) => TopUpOutcome::Denied {
                reason: format!(
                    "requested depth {} exceeds policy max {max_depth} for {:?}/{:?}/{:?}",
                    request.requested_depth, self.tier, self.class, request.kind
                ),
            },
            None => TopUpOutcome::Denied {
                reason: format!(
                    "no top-up policy for {:?}/{:?}/{:?}",
                    self.tier, self.class, request.kind
                ),
            },
        };
        TopUpDecision {
            request: request.clone(),
            outcome,
            source_digest: None,
        }
    }

    /// Record a top-up decision (gate-visible: observable in the pack's own
    /// log) without touching `sections`. Used for denials, and internally
    /// by [`Self::apply_top_up`] for allowances.
    pub fn record_top_up_decision(&mut self, decision: TopUpDecision) {
        self.top_up_log.push(decision);
    }

    /// The ONLY way a section can be added after `pack()`.
    pub fn apply_top_up(
        &mut self,
        decision: TopUpDecision,
        section_source: SourceSlice,
        policy: &TopUpPolicy,
    ) -> Result<(), BriefcaseError> {
        if !matches!(decision.outcome, TopUpOutcome::Allowed) {
            return Err(BriefcaseError::TopUpNotAllowed(
                decision.request.section_key.clone(),
            ));
        }
        if self
            .top_up_log
            .iter()
            .any(|prior| prior.request.request_id == decision.request.request_id)
        {
            return Err(BriefcaseError::TopUpReplay(decision.request.request_id));
        }
        if section_source.key != decision.request.section_key {
            return Err(BriefcaseError::TopUpSourceMismatch);
        }
        let source_digest = digest_of(&section_source.payload);
        if decision.source_digest.as_ref() != Some(&source_digest) {
            return Err(BriefcaseError::TopUpSourceMismatch);
        }
        let key = format!(
            "{:?}:{}",
            decision.request.kind, decision.request.section_key
        )
        .to_lowercase();
        if decision.request.requested_depth < section_source.minimum_depth {
            return Err(BriefcaseError::TopUpDepthExceeded);
        }
        let is_new_key = !self.sections.iter().any(|section| section.key == key);
        let content_count = self
            .sections
            .iter()
            .filter(|section| matches!(section.kind, SectionKind::Preference | SectionKind::Skill))
            .count();
        let ceiling = policy
            .max_total_sections_for(self.tier, self.class)
            .unwrap_or(self.depth);
        if is_new_key && content_count >= ceiling as usize {
            return Err(BriefcaseError::TopUpDepthExceeded);
        }
        let new_section = BriefcaseSection {
            key: key.clone(),
            kind: decision.request.kind,
            visibility: VisibilityClass::WorkerScratch,
            depth: decision.request.requested_depth,
            disclosure_class: Some(crate::disclosure_policy::DisclosureClass::Sensitive),
            payload: section_source.payload,
        };
        if let Some(existing) = self.sections.iter_mut().find(|s| s.key == key) {
            *existing = new_section;
        } else {
            self.sections.push(new_section);
            self.sections.sort_by(|a, b| a.key.cmp(&b.key));
        }
        self.record_top_up_decision(decision);
        Ok(())
    }
}
