from pathlib import Path
import subprocess

ROOT = Path('crates/openspine-kernel')
EXPECTED = {
 'src/package.rs': '6fc21a7291d85fd3b737decd0c84bfb4f11a174a',
 'src/package/source.rs': 'f6e54e7d7ca0bc70f9a46626922c01a16fed1ee1',
 'src/cli/package.rs': '6658c99976d91d52678c37664da241466bf07339',
 'src/cli/mod.rs': '8040ee75bead57f462492c026601557f340a29e7',
 'src/main.rs': 'f5dc65dd756225f5f76e469bed2b854ccc0341e3',
 'src/store/mod.rs': '18e18a3ebe3dc7e0bfd9ccc321689214ab8d36c2',
 'src/store/migrations.rs': '44cd0104d46ede33d0bb7d27539cfb1caedb3829',
 'src/overlay_export_restore/operation/mod.rs': 'ae579d57a6e9a704d8df2e022de7662e3a0a1fdc',
}
for path, expected in EXPECTED.items():
 actual = subprocess.check_output(['git', 'hash-object', str(ROOT/path)], text=True).strip()
 if actual != expected: raise RuntimeError(f'Stale base for {path}: {actual}')

def patch(path, before, after):
 p = ROOT/path
 s = p.read_text()
 if s.count(before) != 1: raise RuntimeError(f'Exact context not unique: {path}: {before!r}')
 p.write_text(s.replace(before, after))

patch('src/package.rs', 'mod validation;', '''mod validation;
pub(crate) mod install_types;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) mod install;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) mod object_store;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod object_fs;
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
#[path = "package/install_tests.rs"]
mod install_tests;''')
patch('src/package.rs', '    let files = source::capture(directory)?;\n    let declaration', '''    from_files(source::capture(directory)?)
}

fn from_files(files: BTreeMap<String, Vec<u8>>) -> Result<PackageSnapshot, InspectionError> {
    let declaration''')
patch('src/package.rs', 'impl PackageSnapshot {', '''impl PackageSnapshot {
    pub(crate) fn identity(&self) -> install_types::PackageIdentity {
        install_types::PackageIdentity {
            package_id: self.report.package_id.clone(),
            revision: self.report.revision,
            inventory_format_version: self.report.inventory_format_version,
            content_digest: self.report.content_digest.clone(),
            manifest_digest: digest_of_bytes(&self.files["package.yaml"]),
        }
    }
''')
patch('src/package/source.rs', '    let mut capture = Capture {', '''    capture_opened(&root)
}

pub(super) fn capture_opened(root: &File) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    // Independent directory cursor: fdopendir's duplicate shares its offset.
    let root = open_at(root, ".", true)?;
    let mut capture = Capture {''')
patch('src/package/source.rs', 'fn open_at(', 'pub(super) fn open_at(')
patch('src/package/source.rs', 'fn names(', 'pub(super) fn names(')
patch('src/cli/package.rs', 'use std::path::PathBuf;', 'use std::path::{Path, PathBuf};')
patch('src/cli/package.rs', 'pub(crate) enum PackageCommands {', '''pub(crate) enum PackageCommands {
    /// List retained installations, all explicitly inactive and unselected.
    List { #[arg(long)] json: bool },
    /// Inspect content-free durable package operation receipts.
    Receipts { #[arg(long)] json: bool },''')
patch('src/cli/package.rs', 'pub(crate) fn run(command: &PackageCommands) -> ExitCode {\n    let PackageCommands::Inspect { directory, json } = command;', '''pub(crate) fn run(command: &PackageCommands, config: &Path) -> ExitCode {
    let (directory, json) = match command {
        PackageCommands::Inspect { directory, json } => (directory, json),
        PackageCommands::List { json } => return super::package_install::list(config, *json, false),
        PackageCommands::Receipts { json } => return super::package_install::list(config, *json, true),
    };''')
patch('src/cli/mod.rs', 'pub(crate) mod package;', 'pub(crate) mod package;\npub(crate) mod package_install;')
patch('src/main.rs', 'enum Commands {', '''enum Commands {
    /// Install an immutable package snapshot without selecting or activating it.
    Install(cli::package_install::InstallArgs),''')
patch('src/main.rs', 'return cli::package::run(command);', 'return cli::package::run(command, &cli.config);')
patch('src/main.rs', '    run_with_runtime(cli)\n}', '''    if let Some(Commands::Install(args)) = &cli.command {
        return cli::package_install::run(args, &cli.config);
    }
    run_with_runtime(cli)
}''')
p=ROOT/'src/store/mod.rs'
s=p.read_text()
start=s.index('    pub fn open(path: &Path)')
end=s.index('\n    }',start)+len('\n    }')
s=s[:start]+'''    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let store = Self::from_connection(Connection::open(path)?)?;
        // Normal runtime startup retains the retroactive dark-window guard.
        store.sweep_ineligible_dark_window_allow_rules(jiff::Timestamp::now())?;
        Ok(store)
    }

    /// Offline maintenance opens only an existing ledger. It neither creates
    /// owner state nor runs the serving path's authority-changing sweep.
    pub(crate) fn open_for_package_management(path: &Path) -> Result<Self, StoreError> {
        let conn = Connection::open_with_flags(path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
            | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
            | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW)?;
        let recognized: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'audit_log')",
            [], |row| row.get(0))?;
        if !recognized {
            return Err(StoreError::BadLedgerMeta("package maintenance requires an existing kernel ledger".into()));
        }
        let store = Self::from_connection(conn)?;
        store.validate_package_ledger()?;
        Ok(store)
    }'''+s[end:]
p.write_text(s)
patch('src/store/mod.rs', '        let mut conn = Connection::open_in_memory()?;', '''        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(mut conn: Connection) -> Result<Self, StoreError> {''')
patch('src/store/mod.rs', 'impl Store {\n    pub fn open(path: &Path)', '''pub(crate) mod package_install;
#[cfg(test)]
mod package_install_tests;

impl Store {
    pub fn open(path: &Path)''')
patch('src/store/migrations.rs', 'pub(super) fn apply_ad_hoc_migrations(conn: &Connection) -> Result<(), StoreError> {', '''pub(super) fn apply_ad_hoc_migrations(conn: &Connection) -> Result<(), StoreError> {
    super::package_install::ensure_schema(conn)?;''')
patch('src/overlay_export_restore/operation/mod.rs', '    pub(crate) fn canonical_data_root(&self) -> &Path {', '''    /// Maintenance refuses pending export/restore instead of applying it.
    pub(crate) fn has_pending_operation(&self) -> Result<bool, ControlError> {
        Ok(self.control.load_operation()?.is_some())
    }

    pub(crate) fn canonical_data_root(&self) -> &Path {''')
p=ROOT/'tests/package_install_cli.rs';s=p.read_text()
old='''        fixture
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_openspine"))'''
new='''        copy_tree(&fixture.root.path().join("candidate"), &fixture.root.path().join("artifacts/lyra"));
        fixture
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_openspine"));
        command'''
assert s.count(old)==1;s=s.replace(old,new)
old='''            .env_remove("OPENSPINE_LOCAL_API_KEY")
            .output()
            .unwrap()'''
new='''            .env_remove("OPENSPINE_LOCAL_API_KEY")
            .env_remove("OPENSPINE_TEST_PACKAGE_CRASH");
        command'''
assert s.count(old)==1;s=s.replace(old,new)
s+='\n#[path = "package_install/faults.rs"]\nmod faults;\n#[path = "package_install/guardrails.rs"]\nmod guardrails;\n';p.write_text(s)
