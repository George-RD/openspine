//! Owner-facing language for reviewed delegation proposals.

use openspine_schemas::standing_rule::StandingRuleManifest;

const MINUTE_SECONDS: i64 = 60;
const HOUR_SECONDS: i64 = 60 * MINUTE_SECONDS;
const DAY_SECONDS: i64 = 24 * HOUR_SECONDS;
const WEEK_SECONDS: i64 = 7 * DAY_SECONDS;

/// Render one reviewed responsibility proposal without exposing runtime ontology.
pub(super) fn render_standing_rule_proposal(
    rule: &StandingRuleManifest,
    eval_summary: &str,
) -> String {
    let action_id = rule.action_id.as_str();
    let email_draft = action_id == "email.create_draft";
    let description = owner_description(rule, email_draft);
    let action_phrase = if email_draft {
        "create Gmail drafts".to_string()
    } else {
        format!("run `{action_id}`")
    };
    let blocked = if email_draft {
        "Sending email remains blocked. Anything outside this draft responsibility still follows its existing approval or deny boundary."
    } else {
        "Anything outside this responsibility still follows its existing approval or deny boundary."
    };

    format!(
        concat!(
            "Lyra noticed you approved the same kind of work more than once.\n\n",
            "Proposed responsibility\n",
            "{description}\n\n",
            "What would happen next\n",
            "For matching work, Lyra may {action_phrase} without asking again each time.\n\n",
            "Limits\n",
            "- Up to {quota_max} {quota_noun} per {quota_period}.\n",
            "- No faster than {rate_max} {rate_noun} per {rate_period}.\n",
            "- This responsibility expires after {expiry} without a successful use.\n\n",
            "What stays blocked\n",
            "{blocked}\n\n",
            "Control\n",
            "You can revoke this responsibility at any time.\n",
            "Approval is bound to the exact reviewed version.\n\n",
            "Review checks\n",
            "{eval_summary}\n\n",
            "Approve to let Lyra take on this limited responsibility."
        ),
        description = description,
        action_phrase = action_phrase,
        quota_max = rule.quota.max,
        quota_noun = usage_noun(rule.quota.max, email_draft),
        quota_period = budget_period(rule.quota.window_secs),
        rate_max = rule.rate.max,
        rate_noun = usage_noun(rule.rate.max, email_draft),
        rate_period = budget_period(rule.rate.window_secs),
        expiry = duration_label(rule.expires_after_secs),
        blocked = blocked,
        eval_summary = eval_summary.trim(),
    )
}

fn owner_description(rule: &StandingRuleManifest, email_draft: bool) -> String {
    let description = rule.description.trim();
    let miner_suffix = format!(" ({})", rule.action_id);
    let miner_generated = description
        .strip_prefix("Recurring owner approval of ")
        .and_then(|remaining| remaining.strip_suffix(miner_suffix.as_str()))
        .is_some();

    if !description.is_empty() && !miner_generated {
        return description.to_string();
    }

    if email_draft {
        "Prepare Gmail drafts for the same kind of work you have already approved.".to_string()
    } else {
        format!(
            "Repeat `{}` only for matching work you have already approved.",
            rule.action_id
        )
    }
}

fn usage_noun(count: u32, email_draft: bool) -> &'static str {
    match (email_draft, count) {
        (true, 1) => "draft",
        (true, _) => "drafts",
        (false, 1) => "use",
        (false, _) => "uses",
    }
}

fn budget_period(seconds: i64) -> String {
    match seconds {
        WEEK_SECONDS => "week".to_string(),
        DAY_SECONDS => "day".to_string(),
        HOUR_SECONDS => "hour".to_string(),
        MINUTE_SECONDS => "minute".to_string(),
        _ => duration_label(seconds),
    }
}

fn duration_label(seconds: i64) -> String {
    let (amount, singular, plural) = if seconds % DAY_SECONDS == 0 {
        (seconds / DAY_SECONDS, "day", "days")
    } else if seconds % HOUR_SECONDS == 0 {
        (seconds / HOUR_SECONDS, "hour", "hours")
    } else if seconds % MINUTE_SECONDS == 0 {
        (seconds / MINUTE_SECONDS, "minute", "minutes")
    } else {
        (seconds, "second", "seconds")
    };
    let unit = if amount == 1 { singular } else { plural };
    format!("{amount} {unit}")
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
            "This version is limited by action and budgets only.",
            "It is not locked to one contact or mailbox.",
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
        assert!(!rendered.contains("For matching work"));

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

        assert!(rendered
            .contains("Prepare Gmail drafts for the same kind of work you have already approved."));
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
