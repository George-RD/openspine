//! Deterministic filesystem substitutions at the production read boundary.
//! The hook is thread-local and compiled only into unit tests; no sleeps,
//! environment switches, background writers or release failpoints are used.
use super::*;
use std::cell::RefCell;
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{symlink, PermissionsExt as _};

thread_local! {
    static AFTER_READ: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

pub(super) fn after_read() {
    let hook = AFTER_READ.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

struct ResetHook;
impl Drop for ResetHook {
    fn drop(&mut self) {
        let _ = AFTER_READ.with(|slot| slot.borrow_mut().take());
    }
}

fn capture_during(
    source: &Path,
    change: impl FnOnce() + 'static,
) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    AFTER_READ.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(change));
    });
    let _reset = ResetHook;
    let result = capture(source);
    assert!(
        AFTER_READ.with(|slot| slot.borrow().is_none()),
        "fixture must reach the source read boundary"
    );
    result
}

fn fixture(family: bool) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let parent = if family {
        source.join("agents")
    } else {
        source.clone()
    };
    fs::create_dir_all(&parent).unwrap();
    let member = parent.join(if family { "a.yaml" } else { "a.md" });
    fs::write(&member, b"captured bytes\n").unwrap();
    (root, source, member)
}

#[test]
fn source_root_replacement_during_capture_is_refused() {
    let (root, source, _) = fixture(false);
    let path = source.clone();
    let saved = root.path().join("saved");
    assert!(capture_during(&source, move || {
        fs::rename(&path, &saved).unwrap();
        fs::create_dir(&path).unwrap();
        fs::write(path.join("a.md"), b"captured bytes\n").unwrap();
    })
    .is_err());
}

#[test]
fn source_root_symlink_substitution_during_capture_is_refused() {
    let (root, source, _) = fixture(false);
    let path = source.clone();
    let saved = root.path().join("saved");
    assert!(capture_during(&source, move || {
        fs::rename(&path, &saved).unwrap();
        symlink(&saved, &path).unwrap();
    })
    .is_err());
}

#[test]
fn source_family_replacement_with_identical_members_is_refused() {
    let (root, source, member) = fixture(true);
    let family = member.parent().unwrap().to_owned();
    let saved = root.path().join("saved-family");
    assert!(capture_during(&source, move || {
        fs::rename(&family, &saved).unwrap();
        fs::create_dir(&family).unwrap();
        fs::write(family.join("a.yaml"), b"captured bytes\n").unwrap();
    })
    .is_err());
}

#[test]
fn source_family_symlink_substitution_during_capture_is_refused() {
    let (root, source, member) = fixture(true);
    let family = member.parent().unwrap().to_owned();
    let saved = root.path().join("saved-family");
    assert!(capture_during(&source, move || {
        fs::rename(&family, &saved).unwrap();
        symlink(&saved, &family).unwrap();
    })
    .is_err());
}

#[test]
fn source_root_membership_growth_during_capture_is_refused() {
    let (_root, source, _) = fixture(false);
    let extra = source.join("b.md");
    assert!(capture_during(&source, move || {
        fs::write(extra, "new source member").unwrap();
    })
    .is_err());
}

#[test]
fn source_family_membership_growth_during_capture_is_refused() {
    let (_root, source, member) = fixture(true);
    let extra = member.with_file_name("b.yaml");
    assert!(capture_during(&source, move || {
        fs::write(extra, "new source member").unwrap();
    })
    .is_err());
}

#[test]
fn source_member_removal_during_capture_is_refused() {
    let (_root, source, member) = fixture(false);
    assert!(capture_during(&source, move || {
        fs::remove_file(member).unwrap();
    })
    .is_err());
}

#[test]
fn source_file_replacement_with_identical_bytes_is_refused() {
    let (_root, source, member) = fixture(false);
    assert!(capture_during(&source, move || {
        fs::remove_file(&member).unwrap();
        fs::write(member, b"captured bytes\n").unwrap();
    })
    .is_err());
}

#[test]
fn source_file_growth_during_read_is_refused() {
    let (_root, source, member) = fixture(false);
    assert!(capture_during(&source, move || {
        fs::OpenOptions::new()
            .append(true)
            .open(member)
            .unwrap()
            .write_all(b"appended bytes\n")
            .unwrap();
    })
    .is_err());
}

#[test]
fn source_file_hardlink_added_during_read_is_refused() {
    let (root, source, member) = fixture(false);
    let alias = root.path().join("outside-source.md");
    assert!(capture_during(&source, move || {
        fs::hard_link(member, alias).unwrap();
    })
    .is_err());
}

#[test]
fn source_file_becoming_executable_during_read_is_refused() {
    let (_root, source, member) = fixture(false);
    assert!(capture_during(&source, move || {
        fs::set_permissions(member, fs::Permissions::from_mode(0o755)).unwrap();
    })
    .is_err());
}

#[test]
fn source_ancestor_retargeted_during_capture_is_refused() {
    let (root, source, _) = fixture(false);
    let alternative = root.path().join("alternative");
    fs::create_dir(&alternative).unwrap();
    fs::write(alternative.join("a.md"), b"captured bytes\n").unwrap();
    let alias = root.path().join("selected");
    symlink(root.path(), &alias).unwrap();
    let configured = alias.join("source");
    let replacement_parent = root.path().join("replacement-parent");
    fs::create_dir(&replacement_parent).unwrap();
    fs::rename(alternative, replacement_parent.join("source")).unwrap();
    assert!(capture_during(&configured, move || {
        fs::remove_file(&alias).unwrap();
        symlink(&replacement_parent, &alias).unwrap();
    })
    .is_err());
    assert!(source.exists());
}

#[test]
fn source_ancestor_retargeted_to_moved_same_inode_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join("original-parent");
    fs::create_dir_all(parent.join("source")).unwrap();
    fs::write(parent.join("source/a.md"), b"captured bytes\n").unwrap();
    let alias = root.path().join("selected");
    symlink(&parent, &alias).unwrap();
    let configured = alias.join("source");
    let moved = root.path().join("moved-parent");
    assert!(capture_during(&configured, move || {
        fs::rename(&parent, &moved).unwrap();
        fs::remove_file(&alias).unwrap();
        symlink(&moved, &alias).unwrap();
    })
    .is_err());
}

#[test]
fn source_stable_ancestor_symlink_remains_supported() {
    let (root, source, _) = fixture(false);
    let alias = root.path().join("selected");
    symlink(root.path(), &alias).unwrap();
    assert_eq!(
        capture(&source).unwrap(),
        capture(&alias.join("source")).unwrap()
    );
}

#[test]
fn source_capture_retains_owned_bytes_after_source_removal() {
    let (_root, source, _) = fixture(false);
    let captured = capture(&source).unwrap();
    fs::remove_dir_all(source).unwrap();
    assert_eq!(captured["a.md"], b"captured bytes\n");
}

#[test]
fn source_capture_opened_uses_independent_directory_cursors() {
    let (_root, source, _) = fixture(true);
    let root = fs::File::open(&source).unwrap();
    let first = capture_opened(&root).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(capture_opened(&root).unwrap(), first);
    assert_eq!(capture(&source).unwrap(), first);
}
