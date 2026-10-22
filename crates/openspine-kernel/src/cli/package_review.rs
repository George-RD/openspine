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
    // Canonical parsing is deliberately before configuration, lock or keys.
    let installation = crate::package::compare::installation_id(installation)?;
    let context = super::package_install::Context::open(config)?;
    let assessment = assess(&context, installation)?;
    Ok(if json {
        assessment.report.json()
    } else {
        assessment.report.summary()
    })
}

/// Everything the accepting command binds, captured under one lifetime lock.
/// Pure reads: this neither selects, approves, recovers nor repairs anything.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) struct Assessment {
    pub report: crate::package::review_report::ReviewReport,
    pub target: crate::package::install_types::InstallReceipt,
    pub source: crate::package::selection_types::SelectionSource,
    /// Selection generation of the captured source (0 = legacy).
    pub generation: u64,
    pub base_epoch_changed: bool,
    /// Learned rows the transition sends back through reconfirmation.
    pub reconfirmations: Vec<crate::package::selection_types::ReconfirmationKey>,
    /// Captured overlay/provenance fingerprint from the shared evaluator.
    pub overlay_fingerprint: openspine_schemas::digest::Digest,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn assess(
    context: &super::package_install::Context,
    installation: ulid::Ulid,
) -> Result<Assessment, Error> {
    use crate::package::{
        current_state::{BaseOrigin, CapturedCurrentState},
        review_overlay, review_report,
        selection_types::{ReconfirmationKey, SelectionSource, SelectionSourceMode},
    };

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
    let current =
        match crate::package::managed_source::resolve(&context.store, context.data_root())
            .map_err(Error::Managed)?
        {
            // Managed history wins: the legacy directory is never opened.
            Some(base) => CapturedCurrentState::capture_snapshot(
                BaseOrigin::Managed {
                    installation_id: base.receipt.installation_id,
                    generation: base.record.generation,
                    provenance: base.receipt.provenance,
                },
                &base.snapshot,
                &receipt.package_id,
                context.data_root(),
                &context.store,
                &artifacts,
            ),
            None => CapturedCurrentState::capture(
                &context.config.lyra_dir,
                &receipt.package_id,
                context.data_root(),
                &context.store,
                &artifacts,
            ),
        }
        .map_err(Error::Current)?;
    let overlays =
        review_overlay::assess(&current, candidate.registry()).map_err(|_| Error::Assessment)?;
    let mut reconfirmations: Vec<_> = overlays
        .after
        .consequences
        .iter()
        .filter(|item| item.reason == "base_epoch_reconfirmation_required")
        .map(|item| ReconfirmationKey {
            kind: item.kind.clone(),
            artifact_id: item.artifact_id.clone(),
            version: item.version,
        })
        .collect();
    reconfirmations.sort();
    reconfirmations.dedup();
    let overlay_fingerprint =
        openspine_schemas::digest::Digest::parse(overlays.control_fingerprint.clone())
            .map_err(|_| Error::Assessment)?;
    let base_epoch_changed = overlays.reusable_authority_reconfirmation_required;
    let (source, generation) = match &current.base.origin {
        BaseOrigin::LegacyConfigured(_) => (
            SelectionSource {
                mode: SelectionSourceMode::LegacyConfigured,
                identity: current.base.identity.clone(),
                installation_id: None,
            },
            0,
        ),
        BaseOrigin::Managed {
            installation_id,
            generation,
            ..
        } => (
            SelectionSource {
                mode: SelectionSourceMode::Managed,
                identity: current.base.identity.clone(),
                installation_id: Some(*installation_id),
            },
            *generation,
        ),
    };
    // Capture expiry-sensitive work once, outside the deterministic evaluator.
    let work = context
        .store
        .package_outstanding_work_with_reviews(jiff::Timestamp::now(), &artifacts)
        .map_err(|_| Error::OutstandingWork)?;
    let report = review_report::build(&current, &candidate, receipt.clone(), overlays, work);
    Ok(Assessment {
        report,
        target: receipt,
        source,
        generation,
        base_epoch_changed,
        reconfirmations,
        overlay_fingerprint,
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn execute(_: &Path, _: &str, _: bool) -> Result<String, Error> {
    Err(InstallError::UnsupportedPlatform.into())
}

pub(super) enum Error {
    Maintenance(InstallError),
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    Managed(crate::package::managed_source::ManagedBaseError),
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
            Self::Managed(error) => {
                use crate::package::managed_source::ManagedBaseError as Managed;
                let (code, integrity, compatibility) = match error {
                    Managed::Ledger => {
                        ("selection-ledger-invalid", "not-evaluated", "not-evaluated")
                    }
                    Managed::ObjectCorrupt => (
                        "selected-object-corrupt",
                        "unavailable-or-corrupt",
                        "not-evaluated",
                    ),
                    Managed::Incompatible(_) => {
                        ("selected-package-incompatible", "verified", "incompatible")
                    }
                };
                (code, error.to_string(), integrity, compatibility)
            }
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

    pub(super) fn render(&self, json: bool) -> String {
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
