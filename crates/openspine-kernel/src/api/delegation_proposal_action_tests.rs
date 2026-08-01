use openspine_schemas::action::ActionId;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::standing_rule::{BudgetWindow, StandingRuleManifest};

use super::delegation_proposal::render_standing_rule_proposal;

fn mined_rule(action_id: &str) -> StandingRuleManifest {
    StandingRuleManifest {
        id: "recurring_owner_work".to_string(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Proposed,
        action_id: ActionId::new(action_id),
        description: format!("Recurring owner approval of opaque_digest ({action_id})"),
        quota: BudgetWindow {
            max: 2,
            window_secs: 24 * 60 * 60,
        },
        rate: BudgetWindow {
            max: 1,
            window_secs: 60 * 60,
        },
        expires_after_secs: 14 * 24 * 60 * 60,
        dark_window: None,
    }
}

#[test]
fn scoped_memory_read_keeps_the_object_and_scope() {
    let rendered = render_standing_rule_proposal(&mined_rule(
        "memory.read:owner_preferences_limited",
    ));

    assert!(rendered.contains("read the limited owner preferences stored in memory"));
    assert!(!rendered.contains("memory.read:owner_preferences_limited"));
    assert!(!rendered.contains("Lyra may read without"));
}

#[test]
fn task_scratch_write_keeps_the_object_and_scope() {
    let rendered =
        render_standing_rule_proposal(&mined_rule("artifact.write:task_scratch"));

    assert!(rendered.contains("write to this task's scratch artifact"));
    assert!(!rendered.contains("artifact.write:task_scratch"));
    assert!(!rendered.contains("Lyra may write without"));
}

#[test]
fn unknown_scoped_action_fallback_retains_all_humanized_context() {
    let rendered =
        render_standing_rule_proposal(&mined_rule("custom.process:limited_workspace"));

    assert!(rendered.contains("run the custom process action for limited workspace"));
    assert!(!rendered.contains("custom.process:limited_workspace"));
    assert!(!rendered.contains("Lyra may process without"));
}
