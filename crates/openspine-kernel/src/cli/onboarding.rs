//! First-start recognition for terminal chat.
//!
//! Completion is runtime state, so it lives under the data directory rather
//! than in `openspine.yaml`: the configuration file belongs to the owner, and
//! the kernel should not be writing its own bookkeeping into it.
//!
//! The notice is driven by readiness and not by the marker alone. Marker-only
//! gating would nag every already-working install exactly once, and would stay
//! silent on an install that is marked complete but has since broken.

use super::readiness::Readiness;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MARKER_FILE: &str = "onboarding.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnboardingState {
    pub schema_version: u32,
    pub completed_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_provider_id: Option<String>,
}

pub fn marker_path(data_dir: &Path) -> PathBuf {
    data_dir.join(MARKER_FILE)
}

/// Whether onboarding has been recorded as complete. A malformed or unreadable
/// marker reads as incomplete: showing the notice again is a smaller failure
/// than suppressing it forever.
pub fn is_complete(data_dir: &Path) -> bool {
    std::fs::read_to_string(marker_path(data_dir))
        .ok()
        .and_then(|text| serde_json::from_str::<OnboardingState>(&text).ok())
        .is_some()
}

pub fn record_complete(
    data_dir: &Path,
    verified_provider_id: Option<&str>,
) -> Result<(), std::io::Error> {
    let state = OnboardingState {
        schema_version: 1,
        completed_at: jiff::Timestamp::now().to_string(),
        verified_provider_id: verified_provider_id.map(str::to_string),
    };
    std::fs::create_dir_all(data_dir)?;
    let encoded = serde_json::to_string_pretty(&state)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    std::fs::write(marker_path(data_dir), encoded)
}

/// What terminal chat should do before its first prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstStart {
    /// Printed before the first prompt, when there is something to say.
    pub notice: Option<String>,
    /// Whether this start should record onboarding as complete.
    pub record_completion: bool,
}

pub fn first_start(readiness: &Readiness, already_complete: bool) -> FirstStart {
    if !readiness.is_ready() {
        return FirstStart {
            notice: Some(blocked_notice(readiness)),
            record_completion: false,
        };
    }
    if already_complete {
        return FirstStart {
            notice: None,
            record_completion: false,
        };
    }
    FirstStart {
        notice: Some(welcome_notice()),
        record_completion: true,
    }
}

fn blocked_notice(readiness: &Readiness) -> String {
    format!(
        "This OpenSpine install is not ready to answer yet.\n\n{}\n\
         Run `openspine setup` to work through these, or `openspine setup --check` \n\
         for the full report. You can still type below, but replies will fail until \n\
         the items above are resolved.\n",
        readiness.render_blocking()
    )
}

fn welcome_notice() -> String {
    "Welcome to OpenSpine. This is Lyra, running under the local governed pipeline.\n\
     \n\
     Every message you send here becomes a verified owner event, runs under a signed \n\
     task grant, and has each effect gated before it happens. Lyra can draft and \n\
     answer; it cannot send mail or reach the network on its own.\n\
     \n\
     Type `/help` for the available commands. This notice appears once.\n"
        .to_string()
}

/// The `/help` body for the chat loop.
pub fn help_text() -> String {
    "Commands:\n\
     \x20 /help    show this list\n\
     \x20 /status  show the readiness report for this install\n\
     \x20 /exit    leave the conversation (Ctrl-D also works)\n\
     \n\
     Configuration lives outside this conversation:\n\
     \x20 openspine setup                  interactive onboarding\n\
     \x20 openspine setup --check          readiness report, no prompts\n\
     \x20 openspine provider login <id>    log in to a model provider\n"
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::readiness::{Check, CheckState};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("openspine-onboard-{tag}-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn ready() -> Readiness {
        Readiness { checks: Vec::new() }
    }

    fn blocked() -> Readiness {
        Readiness {
            checks: vec![Check {
                id: "provider.anthropic".to_string(),
                label: "provider anthropic".to_string(),
                state: CheckState::Fail,
                detail: "no stored OAuth credential".to_string(),
                remedy: Some("run `openspine provider login anthropic`".to_string()),
            }],
        }
    }

    #[test]
    fn a_blocked_install_is_told_what_is_missing_every_start() {
        let first = first_start(&blocked(), false);
        let later = first_start(&blocked(), true);

        for outcome in [&first, &later] {
            let notice = outcome.notice.as_deref().expect("notice");
            assert!(notice.contains("no stored OAuth credential"), "{notice}");
            assert!(
                notice.contains("openspine provider login anthropic"),
                "{notice}"
            );
            assert!(!outcome.record_completion);
        }
    }

    #[test]
    fn a_ready_install_greets_once_then_stays_quiet() {
        let first = first_start(&ready(), false);
        assert!(first.notice.is_some());
        assert!(first.record_completion);

        let later = first_start(&ready(), true);
        assert_eq!(later.notice, None);
        assert!(!later.record_completion);
    }

    #[test]
    fn completion_round_trips_through_the_data_directory() {
        let dir = temp_dir("marker");
        assert!(!is_complete(&dir));

        record_complete(&dir, Some("anthropic")).unwrap();

        assert!(is_complete(&dir));
        let state: OnboardingState =
            serde_json::from_str(&std::fs::read_to_string(marker_path(&dir)).unwrap()).unwrap();
        assert_eq!(state.verified_provider_id.as_deref(), Some("anthropic"));
    }

    /// A truncated marker must not silently suppress the notice forever.
    #[test]
    fn a_corrupt_marker_reads_as_incomplete() {
        let dir = temp_dir("corrupt");
        std::fs::write(marker_path(&dir), "{ not json").unwrap();

        assert!(!is_complete(&dir));
    }

    #[test]
    fn help_names_the_setup_commands() {
        let help = help_text();
        assert!(help.contains("/status"), "{help}");
        assert!(help.contains("openspine setup --check"), "{help}");
        assert!(help.contains("openspine provider login"), "{help}");
    }
}
