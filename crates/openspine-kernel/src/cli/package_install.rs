//! Native, non-serving operator commands. No alternate config, ledger or keys.
use crate::package::install_types::InstallError as Error;
use clap::Args;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Args)]
pub(crate) struct InstallArgs {
    /// Install the package bundled with this runtime (currently lyra).
    #[arg(value_parser = ["lyra"], required_unless_present = "from", conflicts_with = "from")]
    package: Option<String>,
    /// Install this local declarative package without selecting it.
    #[arg(long, required_unless_present = "package")]
    from: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

pub(crate) fn run(args: &InstallArgs, config: &Path) -> ExitCode {
    emit(execute(args, config), args.json)
}

pub(crate) fn list(config: &Path, json: bool, receipts: bool) -> ExitCode {
    emit(list_inner(config, receipts), json)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn execute(args: &InstallArgs, config: &Path) -> Result<serde_json::Value, Error> {
    use crate::package::{install, install_types::PackageProvenance};
    let (source, provenance) = match (&args.package, &args.from) {
        (Some(name), None) if name == "lyra" => (
            super::readiness::default_package_dir(),
            PackageProvenance::RuntimeBundled,
        ),
        (None, Some(directory)) => (directory.clone(), PackageProvenance::LocalUnverified),
        _ => return Err(Error::Configuration),
    };
    // Nothing authoritative is opened until the entire source is validated.
    let snapshot = crate::package::inspect(&source)?;
    let context = Context::open(config)?;
    let result = install::install(&context.store, &context.objects, &snapshot, provenance)?;
    serde_json::to_value(result).map_err(|_| Error::Ledger)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn list_inner(config: &Path, receipts: bool) -> Result<serde_json::Value, Error> {
    let context = Context::open(config)?;
    if receipts {
        crate::package::install::recover(&context.store, &context.objects)?;
        let receipts = context
            .store
            .package_install_receipts()
            .map_err(|_| Error::Ledger)?;
        Ok(serde_json::json!({"schema_version": 1, "receipts": receipts}))
    } else {
        crate::package::install::list(&context.store, &context.objects)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn execute(_: &InstallArgs, _: &Path) -> Result<serde_json::Value, Error> {
    Err(Error::UnsupportedPlatform)
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn list_inner(_: &Path, _: bool) -> Result<serde_json::Value, Error> {
    Err(Error::UnsupportedPlatform)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
struct Context {
    store: crate::store::Store,
    objects: crate::package::object_store::PackageObjects,
    // Dropped last: the regular runtime lock covers the full command lifetime.
    _lock: crate::overlay_export_restore::OverlayOperations,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Context {
    fn open(config: &Path) -> Result<Self, Error> {
        crate::env_file::load_adjacent(config).map_err(|_| Error::Configuration)?;
        let config = crate::config::Config::load(config).map_err(|_| Error::Configuration)?;
        let key = crate::config::artifact_key_bytes().map_err(|_| Error::Configuration)?;
        let mut attempts = 0;
        let lock = loop {
            match crate::overlay_export_restore::acquire(&config.data_dir, &key) {
                Ok(lock) => break lock,
                Err(error)
                    if crate::overlay_export_restore::is_already_locked(&error)
                        && attempts < 50 =>
                {
                    attempts += 1;
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(_) => return Err(Error::Locked),
            }
        };
        if lock.has_pending_operation().map_err(|_| Error::Ledger)? {
            return Err(Error::PendingOperation);
        }
        let root = lock.canonical_data_root();
        let store = crate::store::Store::open_for_package_management(&root.join("kernel.db"))
            .map_err(|_| Error::Ledger)?;
        let objects = crate::package::object_store::PackageObjects::open(root, &config.lyra_dir)?;
        Ok(Self {
            store,
            objects,
            _lock: lock,
        })
    }
}

fn emit(result: Result<serde_json::Value, Error>, json: bool) -> ExitCode {
    let (value, code) = match result {
        Ok(value) => (value, ExitCode::SUCCESS),
        Err(error) => (
            serde_json::json!({"schema_version": 1, "error": {"code": error.code(), "message": error.to_string()}}),
            ExitCode::FAILURE,
        ),
    };
    let output = if json {
        serde_json::to_string(&value)
    } else {
        serde_json::to_string_pretty(&value)
    };
    match output {
        Ok(mut output) => {
            output.push('\n');
            if std::io::stdout()
                .lock()
                .write_all(output.as_bytes())
                .is_ok()
            {
                code
            } else {
                ExitCode::FAILURE
            }
        }
        Err(_) => ExitCode::FAILURE,
    }
}
