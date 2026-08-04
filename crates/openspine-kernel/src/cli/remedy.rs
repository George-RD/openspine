//! Remedies for the startup failures an owner can actually resolve.
//!
//! Startup fails through four subsystems that raise their own error types, so
//! this module matches the failure and returns the one action that clears it.
//! The alternative, a typed startup error enum threaded through configuration,
//! the overlay controller, the secret store, and the listener, is a much larger
//! change than the text justifies.

use crate::cli::readiness;
use crate::config::ConfigError;
use crate::env_file;
use crate::overlay_export_restore::ControlError;
use std::path::Path;

/// The action that resolves `error`, or `None` when the failure has no single
/// obvious remedy and the raw error is the most honest thing to print.
pub fn startup_remedy(error: &anyhow::Error, config_path: &Path) -> Option<String> {
    for cause in error.chain() {
        if let Some(config_error) = cause.downcast_ref::<ConfigError>() {
            return Some(config_remedy(config_error, config_path));
        }
        if let Some(ControlError::AlreadyLocked(_)) = cause.downcast_ref::<ControlError>() {
            return Some(
                "another OpenSpine instance is already using this data directory. \
                 Stop it (an `openspine chat` in another terminal holds the lock \
                 for as long as it runs) and try again."
                    .to_string(),
            );
        }
        if let Some(io_error) = cause.downcast_ref::<std::io::Error>() {
            if io_error.kind() == std::io::ErrorKind::AddrInUse {
                return Some(format!(
                    "the kernel bind address is already in use. Stop the process holding it, \
                     or set a different `kernel.bind_addr` in {}.",
                    config_path.display()
                ));
            }
        }
    }
    None
}

fn config_remedy(error: &ConfigError, config_path: &Path) -> String {
    match error {
        ConfigError::MissingEnv(name) => format!(
            "{name} is not set. Run `openspine setup`, or add {name} to {}.",
            env_file::path_for(config_path).display()
        ),
        ConfigError::InvalidArtifactKey(name, _) => format!(
            "{name} must be 64 hexadecimal characters (32 bytes for AES-256-GCM). \
             Run `openspine setup` to generate one."
        ),
        ConfigError::Read { path, source } if source.kind() == std::io::ErrorKind::NotFound => {
            format!(
                "{} does not exist. Run `openspine setup` to write a starter configuration.",
                path.display()
            )
        }
        ConfigError::Read { path, .. } => format!("check the permissions on {}.", path.display()),
        ConfigError::Parse { path, .. } => format!("fix the YAML in {}.", path.display()),
        ConfigError::InvalidReflectionMinerInterval => format!(
            "set `reflection_miner_interval_seconds` to a positive number in {}.",
            config_path.display()
        ),
    }
}

/// The text [`report_failure`] prints, as a value so it can be asserted on.
///
/// A startup failure exits before chat can report readiness, so the checklist
/// has to come from here: otherwise a missing configuration or absent key
/// material shows one remedy while the other gaps stay invisible until the owner
/// fixes this one and hits the next.
pub fn failure_report(error: &anyhow::Error, config_path: &Path) -> String {
    let mut out = format!("openspine: {error:#}\n");
    if let Some(remedy) = startup_remedy(error, config_path) {
        out.push_str(&format!("\nWhat to do: {remedy}\n"));
    }
    // No vault: key material may be exactly what is missing. Readiness reports
    // OAuth credential state as unchecked rather than guessing.
    let blocking = readiness::assess(config_path, &readiness::process_env, None).render_blocking();
    if !blocking.is_empty() {
        out.push_str("\nBlocking:\n");
        out.push_str(&blocking);
    }
    out.push_str("\nRun `openspine setup --check` for the full readiness report.\n");
    out
}

/// Print `error` with its remedy and the blocking checklist, for `main`.
///
/// Reached by every command, not only startup, so the prefix stays neutral.
pub fn report_failure(error: &anyhow::Error, config_path: &Path) {
    eprint!("{}", failure_report(error, config_path));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn config_path() -> PathBuf {
        PathBuf::from("/etc/openspine/openspine.yaml")
    }

    #[test]
    fn a_held_data_root_lock_names_the_running_instance() {
        let error = anyhow::Error::new(ControlError::AlreadyLocked(PathBuf::from("/data/lock")))
            .context("acquiring overlay operations lifetime lock");

        let remedy = startup_remedy(&error, &config_path()).unwrap();

        assert!(
            remedy.contains("already using this data directory"),
            "{remedy}"
        );
        assert!(remedy.contains("Stop it"), "{remedy}");
    }

    #[test]
    fn a_bound_listener_address_names_the_configuration_key() {
        let error = anyhow::Error::new(std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            "Address already in use (os error 98)",
        ))
        .context("binding 127.0.0.1:7777");

        let remedy = startup_remedy(&error, &config_path()).unwrap();

        assert!(remedy.contains("kernel.bind_addr"), "{remedy}");
        assert!(remedy.contains("openspine.yaml"), "{remedy}");
    }

    #[test]
    fn absent_key_material_names_the_variable_and_the_env_file() {
        let error = anyhow::Error::new(ConfigError::MissingEnv(
            "OPENSPINE_ARTIFACT_KEY".to_string(),
        ))
        .context("loading /etc/openspine/openspine.yaml");

        let remedy = startup_remedy(&error, &config_path()).unwrap();

        assert!(remedy.contains("OPENSPINE_ARTIFACT_KEY"), "{remedy}");
        assert!(remedy.contains("/etc/openspine/openspine.env"), "{remedy}");
    }

    /// `ConfigError::Read` carries an inner `io::Error`, so a naive
    /// io-kind-first match would classify a missing file as something else.
    #[test]
    fn an_absent_configuration_file_points_at_setup() {
        let error = anyhow::Error::new(ConfigError::Read {
            path: config_path(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "No such file or directory"),
        })
        .context("loading /etc/openspine/openspine.yaml");

        let remedy = startup_remedy(&error, &config_path()).unwrap();

        assert!(remedy.contains("openspine setup"), "{remedy}");
    }

    /// A startup failure exits before chat can assess readiness, so this report
    /// is the only place the owner sees the whole picture. Showing one remedy
    /// while the other gaps stay hidden makes them fix one thing, hit the next,
    /// and repeat.
    #[test]
    fn a_startup_failure_lists_every_blocking_check_not_only_the_first_remedy() {
        let dir = std::env::temp_dir().join(format!("openspine-report-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("openspine.yaml");
        let error = anyhow::Error::new(ConfigError::Read {
            path: config_path.clone(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "No such file or directory"),
        })
        .context("loading the configuration");

        let report = failure_report(&error, &config_path);

        assert!(report.contains("What to do:"), "{report}");
        assert!(report.contains("Blocking:"), "{report}");
        // The absent configuration and all three absent keys, not just the first.
        for id in ["config", "key.artifact", "key.grant", "key.webhook"] {
            assert!(report.contains(id), "{id} missing from:\n{report}");
        }
        assert!(report.contains("openspine setup --check"), "{report}");
    }

    #[test]
    fn an_unmatched_failure_has_no_invented_remedy() {
        let error = anyhow::anyhow!("the disk caught fire");

        assert!(startup_remedy(&error, &config_path()).is_none());
    }
}
