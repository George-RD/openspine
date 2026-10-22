//! `openspine use`: owner-reviewed package selection (ANO-822).
//!
//! A non-serving local operator command. It authenticates only the local
//! device boundary (OS account owning the protected instance root and keys plus
//! the persisted owner), uses the terminal as a consent transport - not as proof
//! of identity or human presence - and commits through one Store transaction.
use clap::Args;
use std::path::Path;
use std::process::ExitCode;

#[derive(Debug, Args)]
pub(crate) struct UseArgs {
    /// Exact installation ID from `package list` or an installation receipt.
    #[arg(allow_hyphen_values = true)]
    installation_id: String,
}

pub(crate) fn run(args: &UseArgs, config: &Path) -> ExitCode {
    match execute(config, &args.installation_id) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}", error.render());
            ExitCode::FAILURE
        }
    }
}

/// Fixed diagnostics: no package payload, parser text or path is echoed.
#[derive(Debug, thiserror::Error)]
enum UseError {
    #[error("`use` needs an interactive terminal for consent; nothing was selected.")]
    NotInteractive,
    #[error("The instance is not owned and writable only by this account ({0}); nothing was opened or changed.")]
    Preflight(&'static str),
    #[error("The persisted owner could not be verified against the configuration; nothing was selected. Package maintenance never creates or rebinds an owner.")]
    Owner,
    #[error("The signed erasure ledger does not match the persisted erasure state; nothing was selected. Start the runtime once so it can reconcile, then review again.")]
    Erasure,
    #[error("The review has blockers and cannot be accepted: {0}. Resolve them through the normal paths and review again.")]
    Blocked(String),
    #[error("Consent was not given; nothing was selected.")]
    Cancelled,
    #[error("The reviewed state changed after consent ({0}); nothing was selected. Review again.")]
    Stale(String),
    #[error("The consent window expired; nothing was selected. Review again.")]
    Expired,
    #[error("The selection was refused: {0}")]
    Refused(crate::package::selection_types::SelectionRefusal),
    #[error("The selection could not be recorded durably; the prior selection is unchanged.")]
    Store,
    #[error("{0}")]
    Review(String),
}

impl UseError {
    fn render(&self) -> String {
        format!("Package selection not performed: {self}")
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn execute(_: &Path, _: &str) -> Result<String, UseError> {
    Err(UseError::Preflight("unsupported platform"))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn execute(config: &Path, installation: &str) -> Result<String, UseError> {
    use super::{package_install::Context, package_review};
    use crate::package::install_types::InstallError;
    use std::io::IsTerminal as _;

    // Canonical parsing and the terminal check come before anything is opened.
    let installation = crate::package::compare::installation_id(installation)
        .map_err(|error| UseError::Review(error.to_string()))?;
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(UseError::NotInteractive);
    }
    // Local-owner preflight precedes env loading and `Context::open`, which can
    // chmod, clean or migrate. Nothing here follows a link or repairs a mode.
    preflight::local_owner(config)?;
    let context =
        Context::open(config).map_err(|error: InstallError| UseError::Review(error.to_string()))?;
    let key = *context.artifact_key();
    let verified = verify_owner(&context, &key)?;
    verify_erasure_continuity(&context)?;
    let first = package_review::assess(&context, installation)
        .map_err(|error| UseError::Review(review_message(&error)))?;
    let controls = context
        .store
        .package_control_fingerprint()
        .map_err(|_| UseError::Store)?;
    if !first.report.offerable() {
        print!("{}", first.report.summary());
        return Err(UseError::Blocked(first.report.blockers().join(", ")));
    }
    let presented_at = jiff::Timestamp::now();
    let expires_at = presented_at + jiff::SignedDuration::from_mins(WINDOW_MINUTES);
    let decision = bind(
        &first,
        &verified,
        &controls,
        ulid::Ulid::new(),
        presented_at,
        expires_at,
    );
    context
        .store
        .record_package_selection_decision(&decision, presented_at)
        .map_err(refusal)?;

    print!("{}", first.report.summary());
    let phrase = format!("select {installation}");
    println!(
        "\nPackage {} revision {} (installation {installation}) would become generation {} of the \
         managed base.\nPlan digest: {}\nThis consent expires in {WINDOW_MINUTES} minutes. It \
         approves the base transition only; it approves no learned artifact, standing rule or \
         connector trust.\nType `{phrase}` and press Enter to confirm; anything else cancels:",
        first.target.package_id,
        first.target.revision,
        first.generation + 1,
        first.report.plan_digest(),
    );
    if read_consent()? != phrase {
        return Err(UseError::Cancelled);
    }
    crate::package::install_types::test_barrier("use-after-consent")
        .map_err(|error| UseError::Review(error.to_string()))?;

    // Recheck everything security-relevant under the same lifetime lock.
    let committed_at = jiff::Timestamp::now();
    let verified = verify_owner(&context, &key)?;
    verify_erasure_continuity(&context)?;
    let second = package_review::assess(&context, installation)
        .map_err(|error| UseError::Review(review_message(&error)))?;
    let controls = context
        .store
        .package_control_fingerprint()
        .map_err(|_| UseError::Store)?;
    let confirmed = bind(
        &second,
        &verified,
        &controls,
        decision.decision_id,
        presented_at,
        expires_at,
    );
    let stale = changed_bindings(&decision, &confirmed);
    if !stale.is_empty() {
        return Err(UseError::Stale(stale.join(", ")));
    }
    let record = context
        .store
        .commit_package_selection(&confirmed, committed_at)
        .map_err(refusal)?;
    Ok(format!(
        "Selected installation {installation} as generation {} (authority invalidation generation \
         {}).\nStart the runtime to load it; nothing was reloaded. The committed selection is now \
         authoritative and `lyra_dir` is no longer read.\n",
        record.generation, record.invalidation_generation,
    ))
}

const WINDOW_MINUTES: i64 = 10;

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn refusal(error: crate::store::StoreError) -> UseError {
    use crate::package::selection_types::SelectionRefusal;
    match error {
        crate::store::StoreError::PackageSelection(SelectionRefusal::Expired) => UseError::Expired,
        crate::store::StoreError::PackageSelection(SelectionRefusal::Mismatch) => {
            UseError::Stale("the presented decision".into())
        }
        crate::store::StoreError::PackageSelection(reason) => UseError::Refused(reason),
        _ => UseError::Store,
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn review_message(error: &super::package_review::Error) -> String {
    // Reuse the review's fixed diagnostic text, never its JSON envelope.
    let rendered = error.render(true);
    serde_json::from_str::<serde_json::Value>(&rendered)
        .ok()
        .and_then(|value| value["error"]["message"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "The transition review could not be produced.".into())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn verify_owner(
    context: &super::package_install::Context,
    key: &[u8; 32],
) -> Result<(String, String), UseError> {
    let owner = context
        .store
        .verified_owner_for_selection(context.config.owner.telegram_user_id)
        .map_err(|_| UseError::Owner)?;
    Ok((
        owner.id.to_string(),
        crate::package::selection_binding::owner_binding(key, owner.id, owner.identity_id),
    ))
}

/// Signed erasure continuity is verified separately where present: every
/// erasure the signed ledger records must be reflected in the Store (and the
/// reverse), otherwise only a runtime start can reconcile it. Read-only.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn verify_erasure_continuity(context: &super::package_install::Context) -> Result<(), UseError> {
    let stored: std::collections::BTreeSet<String> = context
        .store
        .erased_counterparty_ids()
        .map_err(|_| UseError::Erasure)?
        .into_iter()
        .map(|id| id.to_string())
        .collect();
    let signed = context
        .signed_erasures()
        .map_err(|_| UseError::Erasure)?
        .unwrap_or_default();
    if signed == stored {
        Ok(())
    } else {
        Err(UseError::Erasure)
    }
}

/// One bounded line from the terminal. EOF, an unreadable terminal or an
/// oversized line is cancellation.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn read_consent() -> Result<String, UseError> {
    use std::io::{BufRead as _, Read as _};
    let mut line = String::new();
    let read = std::io::stdin()
        .lock()
        .take(512)
        .read_line(&mut line)
        .map_err(|_| UseError::Cancelled)?;
    if read == 0 || !line.ends_with('\n') {
        return Err(UseError::Cancelled);
    }
    Ok(line.trim_end_matches(['\n', '\r']).to_owned())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn bind(
    assessment: &super::package_review::Assessment,
    verified: &(String, String),
    controls: &openspine_schemas::digest::Digest,
    decision_id: ulid::Ulid,
    presented_at: jiff::Timestamp,
    expires_at: jiff::Timestamp,
) -> crate::package::selection_types::SelectionDecision {
    use crate::package::{selection_binding, selection_types::*};
    SelectionDecision {
        schema_version: 1,
        decision_id,
        owner_binding: verified.1.clone(),
        principal: verified.0.clone(),
        source: assessment.source.clone(),
        target: SelectionTarget {
            installation_id: assessment.target.installation_id,
            identity: assessment.target.identity(),
        },
        plan_digest: assessment.report.plan_digest().clone(),
        expected_generation: assessment.generation,
        runtime_compatibility: selection_binding::runtime_compatibility(),
        control_fingerprint: openspine_schemas::digest::digest_of(&serde_json::json!({
            "overlay": assessment.overlay_fingerprint,
            "controls": controls,
        })),
        base_epoch_changed: assessment.base_epoch_changed,
        reconfirmations: assessment.reconfirmations.clone(),
        presented_at,
        expires_at,
    }
}

/// Names of the bindings that no longer match; empty means the review holds.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn changed_bindings(
    presented: &crate::package::selection_types::SelectionDecision,
    confirmed: &crate::package::selection_types::SelectionDecision,
) -> Vec<&'static str> {
    let mut changed = Vec::new();
    for (name, differs) in [
        (
            "owner",
            presented.owner_binding != confirmed.owner_binding
                || presented.principal != confirmed.principal,
        ),
        ("source", presented.source != confirmed.source),
        ("target", presented.target != confirmed.target),
        ("plan", presented.plan_digest != confirmed.plan_digest),
        (
            "generation",
            presented.expected_generation != confirmed.expected_generation,
        ),
        (
            "runtime",
            presented.runtime_compatibility != confirmed.runtime_compatibility,
        ),
        (
            "controls",
            presented.control_fingerprint != confirmed.control_fingerprint,
        ),
        (
            "consequences",
            presented.base_epoch_changed != confirmed.base_epoch_changed
                || presented.reconfirmations != confirmed.reconfirmations,
        ),
    ] {
        if differs {
            changed.push(name);
        }
    }
    changed
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod preflight {
    use super::UseError;
    use std::os::unix::fs::MetadataExt as _;
    use std::path::Path;

    fn deny(reason: &'static str) -> UseError {
        UseError::Preflight(reason)
    }

    /// Existing, non-linked, owned by this account, with no `forbidden` bits.
    fn check(path: &Path, directory: bool, forbidden: u32) -> Result<(), UseError> {
        let metadata = std::fs::symlink_metadata(path).map_err(|_| deny("missing local state"))?;
        if metadata.file_type().is_symlink() {
            return Err(deny("a protected path is a link"));
        }
        if directory != metadata.is_dir() || (!directory && !metadata.is_file()) {
            return Err(deny("a protected path has the wrong type"));
        }
        // SAFETY: geteuid has no preconditions and cannot fail.
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err(deny("a protected path belongs to another account"));
        }
        if metadata.mode() & forbidden != 0 {
            return Err(deny("a protected path is accessible to other accounts"));
        }
        Ok(())
    }

    /// OS-account trust boundary only: the state must be owned by this account
    /// and not writable by anyone else (the key file also unreadable). No
    /// environment is loaded, no mode is repaired and no directory is created
    /// before these checks pass.
    pub(super) fn local_owner(config: &Path) -> Result<(), UseError> {
        check(config, false, 0o022)?;
        let env = crate::env_file::path_for(config);
        match std::fs::symlink_metadata(&env) {
            Ok(_) => check(&env, false, 0o077)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(deny("a protected path is unreadable")),
        }
        let loaded =
            crate::config::Config::load(config).map_err(|_| deny("configuration unreadable"))?;
        check(&loaded.data_dir, true, 0o022)?;
        check(&loaded.data_dir.join("kernel.db"), false, 0o022)
    }
}
