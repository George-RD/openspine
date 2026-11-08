//! A lost kernel ledger beside an existing package namespace must refuse,
//! never recreate state or fall back to legacy (ANO-1253).
use super::*;

#[test]
fn deleted_package_ledger_refuses_startup_without_recreating_state() {
    for selected in [false, true] {
        let fixture = Fixture::new();
        let installed = fixture.install();
        if selected {
            let finished = select(&fixture, &id(&installed));
            assert_eq!(finished.code, Some(0), "{}", finished.output);
        }
        let data = fixture.root.path().join("data");
        fs::remove_file(data.join("kernel.db")).unwrap();
        let journal = data.join("kernel.db-journal");
        if journal.exists() {
            fs::remove_file(journal).unwrap();
        }
        let before = files(&data);
        for args in [
            &["chat", "--once", "must not reach a worker"][..],
            &["init"][..],
            &["package", "list", "--json"][..],
        ] {
            let output = fixture
                .command(args)
                .env("RUST_LOG", "info")
                .output()
                .unwrap();
            let transcript = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(!output.status.success(), "{args:?}: {transcript}");
            assert!(
                transcript.contains("package-ledger-missing"),
                "{args:?}: {transcript}"
            );
            assert!(
                transcript.contains("separate empty data_dir"),
                "{args:?}: {transcript}"
            );
            assert!(!data.join("kernel.db").exists(), "{args:?}: {transcript}");
            assert_eq!(files(&data), before);
        }
    }
}
