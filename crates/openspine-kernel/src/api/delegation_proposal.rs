//! Owner-facing language for reviewed delegation proposals.

use openspine_schemas::standing_rule::{DarkWindowDefault, StandingRuleManifest};

const MINUTE_SECONDS: i64 = 60;
const HOUR_SECONDS: i64 = 60 * MINUTE_SECONDS;
const DAY_SECONDS: i64 = 24 * HOUR_SECONDS;
const WEEK_SECONDS: i64 = 7 * DAY_SECONDS;

/// Render one reviewed responsibility proposal without exposing runtime ontology.
/// The caller can reach this function only after both proposal checks pass;
/// their detailed evidence remains persisted rather than copied into Telegram.
pub(super) fn render_standing_rule_proposal(rule: &StandingRuleManifest) -> String {
    let action_id = rule.action_id.as_str();
    let email_draft = action_id == "email.create_draft";
    let description = owner_description(rule);
    let action_phrase = owner_action_phrase(action_id);
    let scope_detail = if email_draft {
        "It is not locked to one contact or mailbox."
    } else {
        "It is not locked to a specific account, person, item, or set of details."
    };
    let next_step = format!(
        concat!(
            "Lyra may {action_phrase} without asking again each time.\n\n",
            "Current scope\n",
            "This version is limited by action and budgets only.\n",
            "{scope_detail}"
        ),
        action_phrase = action_phrase,
        scope_detail = scope_detail,
    );
    let dark_window = dark_window_copy(rule);
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
            "{next_step}\n\n",
            "Limits\n",
            "- Up to {quota_max} {quota_noun} per {quota_period}.\n",
            "- No faster than {rate_max} {rate_noun} per {rate_period}.\n",
            "- This responsibility expires after {expiry} without a successful use.\n",
            "{dark_window}\n",
            "What stays blocked\n",
            "{blocked}\n\n",
            "Control\n",
            "Ask Lyra to revoke this responsibility at any time.\n",
            "Approval is bound to the exact reviewed version.\n\n",
            "Review checks\n",
            "OpenSpine replayed prior examples and ran a risk check. Both checks passed.\n\n",
            "Approve to let Lyra take on this limited responsibility."
        ),
        description = description,
        next_step = next_step,
        quota_max = rule.quota.max,
        quota_noun = usage_noun(rule.quota.max, email_draft),
        quota_period = budget_period(rule.quota.window_secs),
        rate_max = rule.rate.max,
        rate_noun = usage_noun(rule.rate.max, email_draft),
        rate_period = budget_period(rule.rate.window_secs),
        expiry = duration_label(rule.expires_after_secs),
        dark_window = dark_window,
        blocked = blocked,
    )
}

fn owner_description(rule: &StandingRuleManifest) -> String {
    let description = rule.description.trim();
    let miner_suffix = format!(" ({})", rule.action_id);
    let miner_generated = description
        .strip_prefix("Recurring owner approval of ")
        .and_then(|remaining| remaining.strip_suffix(miner_suffix.as_str()))
        .is_some();

    if !description.is_empty() && !miner_generated {
        return description.to_string();
    }

    match rule.action_id.as_str() {
        "email.create_draft" => {
            "Prepare Gmail drafts for the same kind of work you have already approved.".to_string()
        }
        "calendar.book_appointment" => {
            "Book calendar appointments you have approved before.".to_string()
        }
        action_id => format!(
            "Let Lyra {} under the action-and-budget limits below.",
            owner_action_phrase(action_id)
        ),
    }
}

fn owner_action_phrase(action_id: &str) -> String {
    match action_id {
        "email.create_draft" => "create Gmail drafts".to_string(),
        "email.send" => "send email".to_string(),
        "email.read_thread:selected_no_attachments" => {
            "read the selected email thread without attachments".to_string()
        }
        "calendar.book_appointment" => "book calendar appointments".to_string(),
        "telegram.reply:owner_channel" => "reply in your Telegram chat".to_string(),
        "terminal.reply:owner_device" => "reply on your device".to_string(),
        "artifact.revoke" => "revoke an approved responsibility".to_string(),
        "workflow.invoke:approved" => "run an approved workflow".to_string(),
        "setup.workflow.start" => "start an approved setup workflow".to_string(),
        "worker.commission" => "assign work to a limited worker".to_string(),
        "worker.report_result" => "record a worker result".to_string(),
        "openspine.status.read" => "check OpenSpine's status".to_string(),
        other => {
            let unqualified = other.split(':').next().unwrap_or(other);
            let leaf = unqualified.rsplit('.').next().unwrap_or(unqualified);
            let words = leaf.replace('_', " ");
            if words.trim().is_empty() {
                "perform this action".to_string()
            } else {
                words
            }
        }
    }
}

fn dark_window_copy(rule: &StandingRuleManifest) -> String {
    let Some(config) = rule.dark_window else {
        return String::new();
    };
    let outcome = match config.default {
        DarkWindowDefault::Allow => "that one request will be allowed.",
        DarkWindowDefault::Deny => "that request will stay blocked.",
    };
    format!(
        concat!(
            "\nOver-limit requests\n",
            "If a request is over these limits, Lyra will ask you first.\n",
            "If you do not respond within {timeout}, {outcome}\n",
            "Other safety checks still apply."
        ),
        timeout = duration_label(config.timeout_secs),
        outcome = outcome,
    )
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
    use openspine_schemas::standing_rule::{
        BudgetWindow, DarkWindowConfig, DarkWindowDefault, StandingRuleManifest,
        STANDING_RULE_DESCRIPTION_MAX_UTF16_UNITS,
    };

    use crate::api::telegram_truncate::TELEGRAM_MAX_MESSAGE_UTF16_UNITS;

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

    fn gmail_proposal(description: &str) -> StandingRuleManifest {
        proposal(
            "email.create_draft",
            description,
            BudgetWindow {
                max: 5,
                window_secs: 7 * 24 * 60 * 60,
            },
            BudgetWindow {
                max: 1,
                window_secs: 60 * 60,
            },
            90 * 24 * 60 * 60,
        )
    }

    #[test]
    fn gmail_draft_proposal_explains_the_responsibility_and_its_limits() {
        let rule = gmail_proposal("Prepare a reply draft for this known relationship");

        let rendered = render_standing_rule_proposal(&rule);

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
            "Ask Lyra to revoke this responsibility at any time.",
            "exact reviewed version",
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
    fn review_result_stays_plain_and_contains_no_eval_internals() {
        let rule = gmail_proposal("Prepare a reply draft");

        let rendered = render_standing_rule_proposal(&rule);

        assert!(rendered.contains(
            "OpenSpine replayed prior examples and ran a risk check. Both checks passed."
        ));
        for leaked in [
            "AD-142",
            "overlay eval gate",
            "risk judge",
            "{\"turns\"",
            "{\"catalog\"",
        ] {
            assert!(
                !rendered.contains(leaked),
                "leaked `{leaked}` in:\n{rendered}"
            );
        }
    }

    #[test]
    fn allow_dark_window_discloses_the_timeout_and_default() {
        let mut rule = gmail_proposal("Prepare a reply draft");
        rule.dark_window = Some(DarkWindowConfig {
            timeout_secs: 30 * 60,
            default: DarkWindowDefault::Allow,
        });

        let rendered = render_standing_rule_proposal(&rule);

        assert!(rendered.contains("If a request is over these limits, Lyra will ask you first."));
        assert!(rendered.contains(
            "If you do not respond within 30 minutes, that one request will be allowed."
        ));
    }

    #[test]
    fn deny_dark_window_discloses_that_silence_stays_blocked() {
        let mut rule = gmail_proposal("Prepare a reply draft");
        rule.dark_window = Some(DarkWindowConfig {
            timeout_secs: 10 * 60,
            default: DarkWindowDefault::Deny,
        });

        let rendered = render_standing_rule_proposal(&rule);

        assert!(rendered.contains("If a request is over these limits, Lyra will ask you first."));
        assert!(rendered
            .contains("If you do not respond within 10 minutes, that request will stay blocked."));
    }

    #[test]
    fn maximum_valid_description_keeps_the_complete_message_within_telegram_limit() {
        let description = "😀".repeat(STANDING_RULE_DESCRIPTION_MAX_UTF16_UNITS / 2);
        assert_eq!(
            description.encode_utf16().count(),
            STANDING_RULE_DESCRIPTION_MAX_UTF16_UNITS
        );
        let mut rule = gmail_proposal(&description);
        rule.dark_window = Some(DarkWindowConfig {
            timeout_secs: 30 * 60,
            default: DarkWindowDefault::Allow,
        });

        let rendered = render_standing_rule_proposal(&rule);

        assert!(rendered.contains(&description));
        assert!(
            rendered.encode_utf16().count() <= TELEGRAM_MAX_MESSAGE_UTF16_UNITS,
            "owner message exceeded Telegram limit: {} units",
            rendered.encode_utf16().count()
        );
    }

    #[test]
    fn mined_gmail_description_does_not_expose_internal_workflow_ids() {
        let rule = gmail_proposal(
            "Recurring owner approval of selected_thread_email_reply_draft (email.create_draft)",
        );

        let rendered = render_standing_rule_proposal(&rule);

        assert!(rendered
            .contains("Prepare Gmail drafts for the same kind of work you have already approved."));
        assert!(!rendered.contains("selected_thread_email_reply_draft"));
        assert!(!rendered.contains("(email.create_draft)"));
    }

    #[test]
    fn non_email_proposal_uses_plain_action_language_and_discloses_action_only_scope() {
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

        let rendered = render_standing_rule_proposal(&rule);

        assert!(rendered.contains("book calendar appointments"));
        assert!(!rendered.contains("calendar.book_appointment"));
        assert!(!rendered.contains("For matching work"));
        assert!(rendered.contains("This version is limited by action and budgets only."));
        assert!(rendered
            .contains("It is not locked to a specific account, person, item, or set of details."));
        assert!(rendered.contains("Up to 2 uses per day."));
        assert!(rendered.contains("No faster than 1 use per hour."));
        assert!(rendered.contains("expires after 14 days without a successful use."));
        assert!(!rendered.contains("Gmail"));
        assert!(!rendered.contains("Sending email"));
    }

    #[test]
    fn mined_non_email_description_does_not_fall_back_to_a_raw_action_id() {
        let rule = proposal(
            "calendar.book_appointment",
            "Recurring owner approval of opaque_digest (calendar.book_appointment)",
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

        let rendered = render_standing_rule_proposal(&rule);

        assert!(rendered.contains("Book calendar appointments you have approved before."));
        assert!(!rendered.contains("opaque_digest"));
        assert!(!rendered.contains("calendar.book_appointment"));
    }
}
