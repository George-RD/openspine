//! AD-042 skill selection: deterministic index (task class → skills) plus a
//! semantic-matcher fallback, selecting ONLY from the approved shelf.
//!
//! This module is the matcher. It is the reason the finished-skills
//! containment guarantee holds at the *selection* layer: every function here
//! takes `&Store` and returns `Vec<Skill>` — there is no `&mut`, no install
//! call, no ceremony entry point in scope. The matcher can inject a skill
//! into an agent's context (by returning it), but it cannot create a row, so
//! it can never install. A poisoned skill that somehow reached the shelf is
//! still just opaque `body` text here; selection never re-parses it into
//! authority.

use openspine_schemas::skill::Skill;

use crate::store::skill_read_queries::installed_skills_for_agent_and_pack;
use crate::store::Store;

/// Tokenize a task-class / task-shape key the same way for the deterministic
/// semantic fallback: split on `_` and `-` so `email_reply` and `email_draft`
/// share the `email` token. Duplicated tokens are collapsed so a skill cannot
/// inflate its overlap score by repeating a key.
fn tokens_of(key: &str) -> Vec<&str> {
    let mut toks: Vec<&str> = key.split(['_', '-']).filter(|t| !t.is_empty()).collect();
    toks.sort_unstable();
    toks.dedup();
    toks
}

/// Select installed skills for `agent_id`/`pack_id` whose `task_shape` keys
/// intersect the requested `task_class` (AD-042 deterministic index). When no
/// skill's task shape matches exactly, a deterministic semantic-matcher
/// fallback ranks the visible `Installed` candidates by token overlap and
/// returns any that share at least one token (highest overlap first). The
/// fallback can only ever read the approved shelf — it never installs.
///
/// Every function here takes `&Store` and returns `Vec<Skill>`; there is no
/// `&mut`, no install call, no ceremony entry point in scope. The matcher can
/// inject a skill into an agent's context (by returning it), but it cannot
/// create a row, so it can never install. A poisoned skill that somehow
/// reached the shelf is still just opaque `body` text here; selection never
/// re-parses it into authority.
pub fn select_skills_for_task(
    store: &Store,
    agent_id: &str,
    pack_id: &str,
    task_class: &str,
) -> Result<Vec<Skill>, crate::store::StoreError> {
    let candidates = installed_skills_for_agent_and_pack(store, agent_id, pack_id)?;

    // Primary: exact deterministic index on task_shape. A skill matches when
    // any of its (deduplicated) task shapes equals the requested class. The
    // result is sorted deterministically (id asc, version desc) regardless of
    // SQLite row order.
    let mut exact: Vec<Skill> = candidates
        .iter()
        .filter(|skill| {
            let mut shapes = skill.task_shape.clone();
            shapes.sort();
            shapes.dedup();
            shapes.iter().any(|shape| shape == task_class)
        })
        .cloned()
        .collect();
    if !exact.is_empty() {
        exact.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| b.version.cmp(&a.version)));
        return Ok(exact);
    }

    // Fallback: deterministic token-overlap semantic match among the same
    // approved, visible shelf. Unrelated task classes share zero tokens and
    // are therefore never selected.
    let wanted: std::collections::HashSet<&str> = tokens_of(task_class).into_iter().collect();
    let mut scored: Vec<(usize, Skill)> = candidates
        .iter()
        .map(|skill| {
            // Score = the number of DISTINCT tokens the skill's (deduplicated)
            // task shapes share with the requested class. Counting the
            // intersection cardinality (not per-shape matches) means repeating
            // a token across many distinct shapes cannot inflate the rank.
            let mut skill_tokens: std::collections::HashSet<&str> =
                std::collections::HashSet::new();
            let mut shapes = skill.task_shape.clone();
            shapes.sort();
            shapes.dedup();
            for shape in &shapes {
                for tok in tokens_of(shape) {
                    skill_tokens.insert(tok);
                }
            }
            let score = skill_tokens.intersection(&wanted).count();
            (score, skill.clone())
        })
        .filter(|(score, _)| *score > 0)
        .collect();
    // Deterministic order regardless of SQLite row order: highest overlap
    // first, then stable by (id asc, version desc).
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.id.cmp(&b.1.id))
            .then_with(|| b.1.version.cmp(&a.1.version))
    });
    Ok(scored.into_iter().map(|(_, skill)| skill).collect())
}
