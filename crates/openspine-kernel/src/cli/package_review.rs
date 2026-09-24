//! Read-only transition review. Persistence and runtime startup stay outside.
use crate::package::install_types::InstallError;
use std::io::Write as _;
use std::path::Path;
use std::process::ExitCode;

pub(super) fn run(config: &Path, installation: &str, json: bool) -> ExitCode {
    let (output, code) = match execute(config, installation, json) {
        Ok(output) => (output, ExitCode::SUCCESS),
        Err(error) => (error.render(json), ExitCode::FAILURE),
    };
    match std::io::stdout().lock().write_all(output.as_bytes()) {
        Ok(()) => code,
        Err(_) => ExitCode::FAILURE,
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn execute(config: &Path, installation: &str, json: bool) -> Result<String, Error> {
    use crate::package::{
        compare, current_state::CapturedCurrentState, review_overlay, review_report,
    };

    // Canonical parsing is deliberately before configuration, lock or keys.
    let installation = compare::installation_id(installation)?;
    let context = super::package_install::Context::open(config)?;
    let receipt = context
        .store
        .installed_packages()
        .map_err(|_| InstallError::Ledger)?
        .into_iter()
        .find(|receipt| receipt.installation_id == installation)
        .ok_or(InstallError::InstallationNotFound)?;
    // Byte-integrity capture and current compatibility are distinct stages.
    // Validate consumes the same owned bytes; never reopen a verified path.
    let captured = context.objects.capture(&receipt.identity())?;
    let candidate = captured.validate().map_err(Error::Candidate)?;
    let artifacts = context
        .artifacts_without_recovery()
        .map_err(|_| Error::Artifacts)?;
    let current = CapturedCurrentState::capture(
        &context.config.lyra_dir,
        &receipt.package_id,
        context.data_root(),
        &context.store,
        &artifacts,
    )
    .map_err(Error::Current)?;
    let overlays =
        review_overlay::assess(&current, candidate.registry()).map_err(|_| Error::Assessment)?;
    // Capture expiry-sensitive work once, outside the deterministic evaluator.
    let work = context
        .store
        .package_outstanding_work_with_reviews(jiff::Timestamp::now(), &artifacts)
        .map_err(|_| Error::OutstandingWork)?;
    let report = review_report::build(&current, &candidate, receipt, overlays, work);
    Ok(if json {
        report.json()
    } else {
        report.summary()
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn execute(_: &Path, _: &str, _: bool) -> Result<String, Error> {
    Err(InstallError::UnsupportedPlatform.into())
}

enum Error {
    Maintenance(InstallError),
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    Candidate(crate::package::InspectionError),
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    Current(crate::package::current_state::CurrentStateCaptureError),
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    Artifacts,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    Assessment,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    OutstandingWork,
}

impl From<InstallError> for Error {
    fn from(error: InstallError) -> Self {
        Self::Maintenance(error)
    }
}

impl Error {
    fn detail(&self) -> (&'static str, String, &'static str, &'static str) {
        match self {
            Self::Maintenance(error) => (
                error.code(),
                error.to_string(),
                if matches!(error, InstallError::ObjectCorrupt) {
                    "unavailable-or-corrupt"
                } else {
                    "not-evaluated"
                },
                "not-evaluated",
            ),
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            Self::Candidate(error) => {
                let (code, compatibility) =
                    if matches!(error, crate::package::InspectionError::StagingUnavailable) {
                        ("candidate-staging-unavailable", "not-evaluated")
                    } else {
                        ("candidate-incompatible", "incompatible")
                    };
                (code, error.to_string(), "verified", compatibility)
            }
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            Self::Current(error) => {
                use crate::package::current_state::CurrentStateCaptureError as Capture;
                let (code, message) = match error {
                    Capture::Base(_) => (
                        "current-base-unavailable",
                        "The configured current base could not be captured and validated. No legacy package identity was invented.",
                    ),
                    Capture::PackageIdMismatch => (
                        "package-id-mismatch",
                        "The candidate and configured base declare different products. A separate product/data migration is required.",
                    ),
                    Capture::Overlay | Capture::Control => (
                        "overlay-state-unavailable",
                        "Existing overlay bytes or controls could not be captured. No empty overlay was assumed.",
                    ),
                };
                (code, message.into(), "verified", "typed-loader-compatible")
            }
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            Self::Artifacts => (
                "overlay-state-unavailable",
                "Existing artifact evidence or key storage could not be opened without recovery."
                    .into(),
                "verified",
                "typed-loader-compatible",
            ),
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            Self::Assessment => (
                "overlay-state-unavailable",
                "Captured overlay evidence could not produce a trustworthy assessment.".into(),
                "verified",
                "typed-loader-compatible",
            ),
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            Self::OutstandingWork => (
                "outstanding-work-unavailable",
                "The durable-work census could not be completed. Quiescence was not established."
                    .into(),
                "verified",
                "typed-loader-compatible",
            ),
        }
    }

    fn render(&self, json: bool) -> String {
        let (code, message, integrity, compatibility) = self.detail();
        // Values are fixed diagnostics from our error enums. No parser text,
        // configuration paths, opaque selectors or package payloads escape.
        let value = serde_json::json!({
            "schema_version": 1,
            "status": "review-unavailable",
            "approval": "not-performed",
            "activation_supported": false,
            "candidate_integrity": integrity,
            "candidate_compatibility": compatibility,
            "error": { "code": code, "message": message },
        });
        if json {
            value.to_string() + "\n"
        } else {
            serde_json::to_string_pretty(&value).expect("bounded review error is serializable")
                + "\n"
        }
    }
}
