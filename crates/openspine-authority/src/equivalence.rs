//! Kernel-owned authority-equivalence classes (AD-147, AD-124).
//!
//! A class is a deterministic projection of a kernel-composed [`TaskGrant`].
//! It deliberately excludes grant identity, expiry, tokens, provenance, and
//! egress metadata: class identity is exactly the composed authority tuple
//! named by AD-147. Semantic selection receives only a class-scoped view, so
//! it can choose tastefully without ever returning a member from another
//! authority class.
//!
//! Construction is sealed: the only public candidate builder runs the same
//! `compose_authority` the kernel uses to mint a live grant, so a shell or
//! LLM can never label a non-equivalent artifact with a forged class.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;

use openspine_schemas::action::{ActionCatalog, ActionId};
use openspine_schemas::grant::{GrantLimits, TaskGrant};

use crate::compose::{compose_authority, AuthorityInput};

/// The auditable identity of one authority-equivalence class.
///
/// Built only from a kernel-composed [`TaskGrant`]; the fields are private so
/// callers cannot label candidates with a forged class. Lists are sorted and
/// deduplicated because authority composition emits canonical set-like lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityClassId {
    allowed_actions: Vec<ActionId>,
    approval_required_actions: Vec<ActionId>,
    denied_actions: Vec<ActionId>,
    output_channels: Vec<String>,
    limits: GrantLimits,
}

impl Ord for AuthorityClassId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.allowed_actions
            .cmp(&other.allowed_actions)
            .then_with(|| {
                self.approval_required_actions
                    .cmp(&other.approval_required_actions)
            })
            .then_with(|| self.denied_actions.cmp(&other.denied_actions))
            .then_with(|| self.output_channels.cmp(&other.output_channels))
            .then_with(|| {
                self.limits
                    .max_model_calls
                    .cmp(&other.limits.max_model_calls)
            })
            .then_with(|| self.limits.max_artifacts.cmp(&other.limits.max_artifacts))
            .then_with(|| {
                self.limits
                    .max_runtime_seconds
                    .cmp(&other.limits.max_runtime_seconds)
            })
    }
}

impl PartialOrd for AuthorityClassId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl AuthorityClassId {
    fn from_grant(grant: &TaskGrant) -> Self {
        Self {
            allowed_actions: canonical_actions(&grant.allowed_actions),
            approval_required_actions: canonical_actions(&grant.approval_required_actions),
            denied_actions: canonical_actions(&grant.denied_actions),
            output_channels: canonical_strings(&grant.output_channels),
            limits: grant.limits,
        }
    }

    /// The composed allowed action set.
    pub fn allowed_actions(&self) -> &[ActionId] {
        &self.allowed_actions
    }

    /// The composed approval-required action set.
    pub fn approval_required_actions(&self) -> &[ActionId] {
        &self.approval_required_actions
    }

    /// The composed denied action set.
    pub fn denied_actions(&self) -> &[ActionId] {
        &self.denied_actions
    }

    /// The composed output-channel set.
    pub fn output_channels(&self) -> &[String] {
        &self.output_channels
    }

    /// The composed limits.
    pub fn limits(&self) -> GrantLimits {
        self.limits
    }
}

fn canonical_actions(actions: &[ActionId]) -> Vec<ActionId> {
    let mut result = actions.to_vec();
    result.sort();
    result.dedup();
    result
}

fn canonical_strings(values: &[String]) -> Vec<String> {
    let mut result = values.to_vec();
    result.sort();
    result.dedup();
    result
}

/// A kernel-composed candidate whose authority projection is derived purely
/// from `grant`, never supplied by the shell or an LLM.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthorityCandidate<T> {
    id: String,
    grant: TaskGrant,
    value: T,
    class_id: AuthorityClassId,
}

impl<T> AuthorityCandidate<T> {
    /// Build a candidate from the kernel's composed grant (test/internal only).
    pub(crate) fn from_composed_grant(id: impl Into<String>, grant: TaskGrant, value: T) -> Self {
        let class_id = AuthorityClassId::from_grant(&grant);
        Self {
            id: id.into(),
            grant,
            value,
            class_id,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn grant(&self) -> &TaskGrant {
        &self.grant
    }

    pub fn value(&self) -> &T {
        &self.value
    }

    pub fn class_id(&self) -> &AuthorityClassId {
        &self.class_id
    }
}

/// A deterministic collection of authority-equivalence classes.
#[derive(Debug, PartialEq)]
pub struct AuthorityEquivalenceClasses<T> {
    classes: BTreeMap<AuthorityClassId, Vec<AuthorityCandidate<T>>>,
}

/// Errors found while constructing the auditable class collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EquivalenceError {
    DuplicateCandidateId(String),
    CompositionDenied(String),
}

impl fmt::Display for EquivalenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateCandidateId(id) => write!(f, "duplicate authority candidate id: {id}"),
            Self::CompositionDenied(id) => {
                write!(f, "candidate {id} did not compose an authority grant")
            }
        }
    }
}

impl<T> AuthorityEquivalenceClasses<T> {
    /// Compose each input through the kernel's authority function and group
    /// the resulting grants by their deterministic class identity.
    pub fn compose_all<'a, 'b, I, Id>(
        catalog: &ActionCatalog,
        inputs: I,
        now: jiff::Timestamp,
    ) -> Result<Self, EquivalenceError>
    where
        'a: 'b,
        I: IntoIterator<Item = (Id, &'b AuthorityInput<'a>, T)>,
        Id: Into<String>,
    {
        let mut candidates = Vec::new();
        for (id, input, value) in inputs {
            let id = id.into();
            match compose_authority(input, catalog, now) {
                crate::compose::AuthorityOutcome::Granted(grant) => {
                    candidates.push(AuthorityCandidate::from_composed_grant(id, *grant, value));
                }
                _ => return Err(EquivalenceError::CompositionDenied(id)),
            }
        }
        Self::from_candidates(candidates)
    }

    /// Group already-composed candidates by their kernel-derived class.
    pub fn from_candidates(
        candidates: impl IntoIterator<Item = AuthorityCandidate<T>>,
    ) -> Result<Self, EquivalenceError> {
        let mut candidates: Vec<_> = candidates.into_iter().collect();
        candidates.sort_by(|left, right| left.id.cmp(&right.id));
        for pair in candidates.windows(2) {
            if pair[0].id == pair[1].id {
                return Err(EquivalenceError::DuplicateCandidateId(pair[0].id.clone()));
            }
        }

        let mut classes = BTreeMap::new();
        for candidate in candidates {
            classes
                .entry(candidate.class_id.clone())
                .or_insert_with(Vec::new)
                .push(candidate);
        }
        Ok(Self { classes })
    }

    pub fn class_count(&self) -> usize {
        self.classes.len()
    }

    /// Iterate class views in canonical class-key order.
    pub fn classes(&self) -> impl Iterator<Item = AuthorityClass<'_, T>> {
        self.classes
            .iter()
            .map(|(id, members)| AuthorityClass { id, members })
    }

    /// Return one class by its kernel-derived identity.
    pub fn class(&self, id: &AuthorityClassId) -> Option<AuthorityClass<'_, T>> {
        self.classes
            .get_key_value(id)
            .map(|(id, members)| AuthorityClass { id, members })
    }

    /// Resolve matching classes without allowing a cross-class pick.
    ///
    /// A single known class is selected deterministically. Multiple known
    /// classes escalate; no member is returned for that case. Unknown class
    /// identities are ignored, so a caller cannot inject a shell-created
    /// class into the result.
    pub fn resolve(&self, matching_class_ids: &[AuthorityClassId]) -> ClassResolution<'_, T> {
        let mut known: Vec<AuthorityClassId> = matching_class_ids
            .iter()
            .filter(|id| self.classes.contains_key(*id))
            .cloned()
            .collect();
        known.sort();
        known.dedup();
        match known.len() {
            0 => ClassResolution::NoMatch,
            1 => {
                let id = known.remove(0);
                let Some((id_ref, members)) = self.classes.get_key_value(&id) else {
                    return ClassResolution::NoMatch;
                };
                ClassResolution::Selected(ResolvedAuthorityClass {
                    class: AuthorityClass {
                        id: id_ref,
                        members,
                    },
                })
            }
            _ => ClassResolution::Escalate { class_ids: known },
        }
    }
}

/// A read-only class view passed to the semantic matcher.
///
/// The matcher can inspect only this class's candidates and returns an index;
/// it cannot return an arbitrary artifact from another class.
pub struct AuthorityClassScope<'a, T> {
    id: &'a AuthorityClassId,
    members: &'a [AuthorityCandidate<T>],
}

impl<'a, T> AuthorityClassScope<'a, T> {
    pub fn class_id(&self) -> &'a AuthorityClassId {
        self.id
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    pub fn candidate_ids(&self) -> impl Iterator<Item = &'a str> {
        self.members.iter().map(|candidate| candidate.id())
    }

    pub fn values(&self) -> impl Iterator<Item = &'a T> {
        self.members.iter().map(|candidate| candidate.value())
    }
}

/// A class member created only by [`ResolvedAuthorityClass::select_within_class`].
#[derive(Debug, Clone, Copy)]
pub struct AuthorityClassMember<'a, T> {
    id: &'a AuthorityClassId,
    candidate: &'a AuthorityCandidate<T>,
}

impl<'a, T> AuthorityClassMember<'a, T> {
    pub fn class_id(&self) -> &'a AuthorityClassId {
        self.id
    }

    pub fn candidate_id(&self) -> &'a str {
        self.candidate.id()
    }

    pub fn grant(&self) -> &'a TaskGrant {
        self.candidate.grant()
    }

    pub fn value(&self) -> &'a T {
        self.candidate.value()
    }
}

/// A class view used for deterministic inspection and semantic selection.
pub struct AuthorityClass<'a, T> {
    id: &'a AuthorityClassId,
    members: &'a [AuthorityCandidate<T>],
}

impl<'a, T> AuthorityClass<'a, T> {
    pub fn id(&self) -> &'a AuthorityClassId {
        self.id
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    pub fn candidate_ids(&self) -> impl Iterator<Item = &'a str> {
        self.members.iter().map(|candidate| candidate.id())
    }
}
/// A successful single-class resolution handle. Its class borrow is private;
/// callers can obtain one only from `ClassResolution::Selected`.
pub struct ResolvedAuthorityClass<'a, T> {
    class: AuthorityClass<'a, T>,
}

impl<'a, T> ResolvedAuthorityClass<'a, T> {
    pub fn id(&self) -> &'a AuthorityClassId {
        self.class.id
    }

    pub fn len(&self) -> usize {
        self.class.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.class.members.is_empty()
    }

    /// Semantic selection is available only on a successful unique-class
    /// resolution handle, never on an unselected audit view.
    pub fn select_within_class<F>(&self, chooser: F) -> Option<AuthorityClassMember<'a, T>>
    where
        F: FnOnce(AuthorityClassScope<'a, T>) -> Option<usize>,
    {
        let scope = AuthorityClassScope {
            id: self.class.id,
            members: self.class.members,
        };
        let index = chooser(scope)?;
        self.class
            .members
            .get(index)
            .map(|candidate| AuthorityClassMember {
                id: self.class.id,
                candidate,
            })
    }
}

/// Result of resolving semantic matches across authority classes.
pub enum ClassResolution<'a, T> {
    NoMatch,
    Selected(ResolvedAuthorityClass<'a, T>),
    Escalate { class_ids: Vec<AuthorityClassId> },
}
