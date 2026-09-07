//! Offline package commands. Dispatch before owner environment/config startup.
use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub(crate) enum PackageCommands {
    /// Inspect exact local package bytes without installation or activation.
    Inspect {
        directory: PathBuf,
        /// Print the versioned, content-free inspection report as JSON.
        #[arg(long)]
        json: bool,
    },
}

pub(crate) fn run(command: &PackageCommands) -> ExitCode {
    let PackageCommands::Inspect { directory, json } = command;
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
