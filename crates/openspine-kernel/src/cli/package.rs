//! Offline package commands. Dispatch before owner environment/config startup.
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub(crate) enum PackageCommands {
    /// List retained installations, all explicitly inactive and unselected.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Inspect content-free durable package operation receipts.
    Receipts {
        #[arg(long)]
        json: bool,
    },
    /// Compare two exact installed inventories without selecting or approving them.
    Compare {
        #[arg(allow_hyphen_values = true)]
        from_installation_id: String,
        #[arg(allow_hyphen_values = true)]
        to_installation_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Inspect exact local package bytes without installation or activation.
    Inspect {
        directory: PathBuf,
        /// Print the versioned, content-free inspection report as JSON.
        #[arg(long)]
        json: bool,
    },
}

pub(crate) fn run(command: &PackageCommands, config: &Path) -> ExitCode {
    let (directory, json) = match command {
        PackageCommands::Inspect { directory, json } => (directory, json),
        PackageCommands::List { json } => {
            return super::package_install::list(config, *json, false)
        }
        PackageCommands::Receipts { json } => {
            return super::package_install::list(config, *json, true)
        }
        PackageCommands::Compare {
            from_installation_id,
            to_installation_id,
            json,
        } => {
            return super::package_install::compare(
                config,
                from_installation_id,
                to_installation_id,
                *json,
            )
        }
    };
    let (output, code) = match crate::package::inspect(directory) {
        Ok(snapshot) => {
            let output = if *json {
                // The report contains only bounded ASCII paths/IDs, digests,
                // fixed labels and numbers; no free-form package content.
                serde_json::to_string(snapshot.report()).expect("inspection report is serializable")
                    + "\n"
            } else {
                snapshot.summary()
            };
            (output, ExitCode::SUCCESS)
        }
        Err(error) => {
            let output = if *json {
                serde_json::json!({
                    "schema_version": 1,
                    "valid": false,
                    "provenance": "local-unverified",
                    "error": { "code": error.code(), "message": error.to_string() },
                })
                .to_string()
                    + "\n"
            } else {
                format!("Inspection failed [{}]: {error}\n", error.code())
            };
            (output, ExitCode::FAILURE)
        }
    };
    if std::io::stdout()
        .lock()
        .write_all(output.as_bytes())
        .is_err()
    {
        return ExitCode::FAILURE;
    }
    code
}
