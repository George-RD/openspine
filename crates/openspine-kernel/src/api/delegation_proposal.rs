//! Owner-facing language for reviewed delegation proposals.

use openspine_schemas::standing_rule::StandingRuleManifest;

/// Render one reviewed responsibility proposal without exposing runtime ontology.
pub(super) fn render_standing_rule_proposal(
    _rule: &StandingRuleManifest,
    _eval_summary: &str,
) -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use openspine_schemas::action::ActionId;
    use openspine_schemas::artifact::Lifecycle;
    use openspine_schemas::standing_rule::{BudgetWindow, StandingRuleManifest};

    use super::render_standing_rule_proposal;

    fn proposal(
        action_id: &str,
        description: &str,
        quota: BudgetWindow,
        rate: BudgetWindow,
        expires_after_secs: i64,
    ) -> StandingRuleManifest {
        StandingRuleManifest {
            id: "recurring_owner_work".to_string(),
            schema_version: 1,
            version: 1,
            lifecycle_state: Lifecycle::Proposed,
            action_id: ActionId::new(action_id),
            description: description.to_string(),
            quota,
            rate,
            expires_after_secs,
            dark_window: None,
        }
    }

    #[test]
    fn gmail_draft_proposal_explains_the_responsibility_and_its_limits() {
        let rule = proposal(
            "email.create_draft",
            "Prepare a reply draft for this known relationship",
            BudgetWindow {
                max: 5,
                window_secs: 7 * 24 * 60 * 60,
            },
            BudgetWindow {
                max: 1,
                window_secs: 60 * 60,
            },
            90 * 24 * 60 * 60,
        );

        let rendered = render_standing_rule_proposal(&rule, "Replay and risk checks passed.");

        for expected in [
            "Lyra noticed you approved the same kind of work more than once.",
            "Prepare a reply draft for this known relationship",
            "create Gmail drafts",
            "Up to 5 drafts per week.",
            "No faster than 1 draft per hour.",
            "expires after 90 days without a successful use.",
            "Sending email remains blocked.",
            "revoke this responsibility",
            "exact reviewed version",
            "Replay and risk checks passed.",
        ] {
            assert!(
                rendered.contains(expected),
                "missing `{expected}` in:\n{rendered}"
            );
        }

        let lower = rendered.to_lowercase();
        for internal_term in ["standing rule", "task grant", "artifact lifecycle"] {
            assert!(
                !lower.contains(internal_term),
                "owner copy exposed `{internal_term}` in:\n{rendered}"
            );
        }
    }

    #[test]
    fn mined_gmail_description_does_not_expose_internal_workflow_ids() {
        let rule = proposal(
            "email.create_draft",
            "Recurring owner approval of selected_thread_email_reply_draft (email.create_draft)",
            BudgetWindow {
                max: 5,
                window_secs: 7 * 24 * 60 * 60,
            },
            BudgetWindow {
                max: 1,
                window_secs: 60 * 60,
            },
            90 * 24 * 60 * 60,
        );

        let rendered = render_standing_rule_proposal(&rule, "Checks passed.");

        assert!(rendered.contains(
            "Prepare Gmail drafts for the same kind of work you have already approved."
        ));
        assert!(!rendered.contains("selected_thread_email_reply_draft"));
        assert!(!rendered.contains("(email.create_draft)"));
    }

    #[test]
    fn non_email_proposal_stays_action_specific_without_email_claims() {
        let rule = proposal(
            "calendar.book_appointment",
            "Book routine appointments in the agreed calendar",
            BudgetWindow {
                max: 2,
                window_secs: 24 * 60 * 60,
            },
            BudgetWindow {
                max: 1,
                window_secs: 60 * 60,
            },
            14 * 24 * 60 * 60,
        );

        let rendered = render_standing_rule_proposal(&rule, "Checks passed.");

        assert!(rendered.contains("run `calendar.book_appointment`"));
        assert!(rendered.contains("Up to 2 uses per day."));
        assert!(rendered.contains("No faster than 1 use per hour."));
        assert!(rendered.contains("expires after 14 days without a successful use."));
        assert!(!rendered.contains("Gmail"));
        assert!(!rendered.contains("Sending email"));
    }
}
