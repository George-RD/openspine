//! `skill.context` kernel action (AD-040/AD-042): returns the installed,
//! approved-shelf skills matching a task class for the *authenticated*
//! agent/pack of the `TaskGrant`, inside an explicit `untrusted` envelope.

use serde_json::{json, Value};

use super::actions::DispatchError;
use crate::pipeline::AppState;
use openspine_schemas::grant::TaskGrant;

/// `skill.context`'s real kernel implementation (AD-040/AD-042): select the
/// installed, approved-shelf skills whose task shape matches the requested
/// `task_class` for the *authenticated* agent/pack of the `TaskGrant`, and
/// return their `body` text inside an *untrusted* envelope the shell model
/// must treat as competence data, not instructions. The matcher only ever
/// READS the approved shelf — it can never install — and the skill `body` is
/// opaque capability text (D-011/D-012), so returning it here does not confer
/// authority: the gate still constrains every outbound effect the shell later
/// chooses.
///
/// Authorization scope is NOT taken from the caller: `agent_id` and
/// `pack_id` are derived from the authenticated `TaskGrant` (its bound
/// agent/pack), so a grantee can only ever query the skills its own grant's
/// scope permits — it cannot scope another agent/pack's visibility.
fn task_class_from_grant_purpose(purpose: &str) -> Option<&'static str> {
    match purpose {
        "selected_thread_email_reply_draft" | "email_reply" => Some("email_reply"),
        "calendar_schedule" => Some("calendar_schedule"),
        _ => None,
    }
}

pub(super) async fn dispatch_skill_context(
    state: &AppState,
    grant: &TaskGrant,
    _payload: Option<&Value>,
) -> Result<Value, DispatchError> {
    let agent_id = grant.agent_id.to_string();
    let pack_id = grant.capability_pack_id.to_string();
    // Task class is a canonical mapping from the authenticated grant's
    // structured purpose; caller payload cannot choose a task family.
    let Some(task_class) = task_class_from_grant_purpose(&grant.purpose) else {
        return Err(DispatchError::BadRequest(
            "skill.context requires a recognized grant purpose".to_string(),
        ));
    };

    let skills =
        crate::skill::select_skills_for_task(&state.store, &agent_id, &pack_id, task_class)
            .map_err(|e| DispatchError::Resource(anyhow::Error::new(e)))?;

    // Explicit untrusted-context envelope: the bodies are competence data the
    // shell MAY follow, never instructions it MUST obey. Marking them
    // `untrusted: true` keeps the kernel/shell boundary honest and prevents a
    // poisoned skill body from masquerading as kernel authority.
    let bodies: Vec<Value> = skills
        .iter()
        .map(|s| {
            let token_id = ulid::Ulid::new();
            let selection = crate::store::skill_read_queries::SkillContextSelection {
                id: token_id,
                task_grant_id: grant.id,
                agent_id: agent_id.clone(),
                pack_id: pack_id.clone(),
                skill_id: s.id.clone(),
                skill_version: s.version,
                task_class: task_class.to_string(),
                expires_at: grant.expires_at,
                used: false,
            };
            crate::store::skill_read_queries::insert_skill_context_selection(
                &state.store,
                &selection,
            )
            .map_err(|e| DispatchError::Resource(anyhow::Error::new(e)))?;
            Ok(json!({
                "id": s.id,
                "version": s.version,
                "title": s.title,
                "body": s.body,
                "selection_token_id": token_id,
                "untrusted": true,
            }))
        })
        .collect::<Result<_, DispatchError>>()?;
    Ok(json!({
        "untrusted": true,
        "context_kind": "skill_competence",
        "agent_id": agent_id,
        "pack_id": pack_id,
        "matched": bodies.len(),
        "skills": bodies,
    }))
}

#[cfg(test)]
mod tests {
    use super::task_class_from_grant_purpose;
    use crate::api::actions::DispatchError;
    use crate::skill::ceremony::CeremonyToken;
    use crate::store::skill_store::insert_skill;
    use crate::test_support::fixtures::test_state;
    use jiff::Timestamp;
    use openspine_schemas::skill::{Skill, SkillProvenance, SkillState, SkillVisibility};
    use serde_json::json;

    #[test]
    fn unknown_near_match_purpose_fails_closed() {
        assert_eq!(
            task_class_from_grant_purpose("calendar_email_reply_conflict"),
            None
        );
        assert_eq!(
            task_class_from_grant_purpose("selected_thread_email_reply_draft"),
            Some("email_reply")
        );
    }
    #[tokio::test]
    async fn skill_context_selects_only_grant_scoped_installed_matches() {
        let state = test_state();
        let (grant, _) = super::super::dispatch_tests::mint_grant_with_selection_token(
            &state,
            &["skill.context"],
            Timestamp::now() + std::time::Duration::from_secs(120),
        );
        let body = "trusted competence".to_string();
        let skill = Skill {
            id: "grant_skill".to_string(),
            schema_version: 1,
            version: 1,
            provenance: SkillProvenance::ShippedSeed,
            state: SkillState::Installed,
            title: "Grant skill".to_string(),
            body: body.clone(),
            task_shape: vec!["email_reply".to_string()],
            visibility: SkillVisibility {
                agents: vec![grant.agent_id.to_string()],
                packs: vec![],
            },
            content_digest: Skill::digest_of_body(&body),
        };
        insert_skill(
            &state.store,
            &skill,
            Timestamp::now(),
            &CeremonyToken::test_token(),
        )
        .unwrap();

        let result = super::dispatch_skill_context(
            &state,
            &grant,
            Some(&json!({"task_class": "calendar_schedule"})),
        )
        .await
        .unwrap();
        assert_eq!(result["untrusted"], true);
        assert_eq!(result["agent_id"], grant.agent_id.to_string());
        assert_eq!(result["pack_id"], grant.capability_pack_id.to_string());
        assert_eq!(result["matched"], 1);
        assert_eq!(result["skills"][0]["body"], body);
        assert_eq!(result["skills"][0]["untrusted"], true);
    }
    #[tokio::test]
    async fn skill_context_rejects_unknown_grant_purpose() {
        let state = test_state();
        let (mut grant, _) = super::super::dispatch_tests::mint_grant_with_selection_token(
            &state,
            &["skill.context"],
            Timestamp::now() + std::time::Duration::from_secs(120),
        );
        grant.purpose = "unknown_skill_purpose".to_string();
        let err = super::dispatch_skill_context(&state, &grant, None)
            .await
            .expect_err("unknown grant purpose must fail closed");
        assert!(matches!(err, DispatchError::BadRequest(_)));
    }
}
