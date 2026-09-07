//! Offline inspection must not even construct the asynchronous runtime.
use std::path::Path;
use std::process::Command;

#[test]
fn package_inspect_does_not_construct_async_runtime() {
    let temporary = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/lyra");
    let output = Command::new(env!("CARGO_BIN_EXE_openspine"))
        .env_clear()
        .env("HOME", temporary.path().join("home"))
        // Tokio rejects zero worker threads if its runtime is constructed.
        .env("TOKIO_WORKER_THREADS", "0")
        .current_dir(temporary.path())
        .arg("--config")
        .arg(temporary.path().join("application/openspine.yaml"))
        .args(["package", "inspect"])
        .arg(source)
        .arg("--json")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["valid"], true);
    assert!(!temporary.path().join("home").exists());
    assert!(!temporary.path().join("application").exists());
}
