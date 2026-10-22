//! Where the control directory lives: beside the data root by default, inside a
//! configured volume for containerised deployments.

use super::*;
use acquire::resolve_root_identity_with;

#[test]
fn without_an_override_the_control_dir_stays_beside_the_data_root() {
    let (_root, data) = temp_data_root();
    let identity = resolve_root_identity_with(&data, None).unwrap();
    assert_eq!(
        identity.control_root,
        identity
            .canonical_data_root
            .parent()
            .unwrap()
            .join(&identity.control_root_name)
    );
}

#[test]
fn an_override_moves_only_the_parent_and_keeps_the_name_stable() {
    let (root, data) = temp_data_root();
    let volume = root.path().join("control-volume");
    fs::create_dir_all(&volume).unwrap();
    let native = resolve_root_identity_with(&data, None).unwrap();
    let first = resolve_root_identity_with(&data, Some(&volume)).unwrap();
    let second = resolve_root_identity_with(&data, Some(&volume)).unwrap();
    assert_eq!(first.control_root_name, native.control_root_name);
    assert_eq!(
        first.control_root,
        fs::canonicalize(&volume)
            .unwrap()
            .join(&first.control_root_name)
    );
    assert_eq!(first.control_root, second.control_root);
}

#[test]
fn an_unusable_override_is_refused_not_created() {
    let (root, data) = temp_data_root();
    let missing = root.path().join("not-mounted");
    assert!(resolve_root_identity_with(&data, Some(&missing)).is_err());
    assert!(!missing.exists(), "an absent volume must not be created");
    assert!(matches!(
        resolve_root_identity_with(&data, Some(Path::new("relative"))),
        Err(ControlError::UnsafeControlPath(_))
    ));
    assert!(matches!(
        resolve_root_identity_with(&data, Some(&data)),
        Err(ControlError::UnsafeControlPath(_))
    ));
    let inside = data.join("sub");
    fs::create_dir_all(&inside).unwrap();
    assert!(matches!(
        resolve_root_identity_with(&data, Some(&inside)),
        Err(ControlError::UnsafeControlPath(_))
    ));
    let file = root.path().join("a-file");
    fs::write(&file, b"x").unwrap();
    assert!(resolve_root_identity_with(&data, Some(&file)).is_err());
}

/// The recreate scenario: the data root persists, and so does the control
/// directory because it sits in a volume. A second boot finds the ledger.
#[test]
fn a_relocated_control_dir_carries_continuity_across_boots() {
    let (root, data) = temp_data_root();
    let volume = root.path().join("control-volume");
    fs::create_dir_all(&volume).unwrap();
    let identity = resolve_root_identity_with(&data, Some(&volume)).unwrap();
    fs::create_dir_all(&identity.control_root).unwrap();
    let again = resolve_root_identity_with(&data, Some(&volume)).unwrap();
    assert!(again.control_root.exists());
    // Without the volume the same data root resolves elsewhere: the lost-control
    // situation `MissingContinuity` guards against stays detectable.
    let native = resolve_root_identity_with(&data, None).unwrap();
    assert!(!native.control_root.exists());
}

/// Runs the real acquire + first-boot path under the override: the ledger lands
/// in the volume, not beside the data root, and a second boot reuses it.
#[test]
fn first_boot_under_an_override_keeps_the_ledger_in_the_volume() {
    let (root, data) = temp_data_root();
    let volume = root.path().join("control-volume");
    fs::create_dir_all(&volume).unwrap();

    let control = OverlayControl::acquire_with_control_parent(&data, KEY, Some(&volume)).unwrap();
    control.ensure_data_root_for_first_boot().unwrap();
    let control_root = control.control_root().to_path_buf();
    assert!(control_root.starts_with(fs::canonicalize(&volume).unwrap()));
    assert!(control_root.join(LEDGER_FILE).exists());
    assert!(
        !fs::read_dir(root.path()).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".openspine-control-")),
        "nothing is created beside the data root"
    );
    drop(control);

    let again = OverlayControl::acquire_with_control_parent(&data, KEY, Some(&volume)).unwrap();
    again.ensure_data_root_for_first_boot().unwrap();
}

/// A fresh (or wrong) volume against a populated data root must stop startup
/// rather than start a new ledger.
#[test]
fn a_fresh_override_against_a_populated_data_root_is_missing_continuity() {
    let (root, data) = temp_data_root();
    let first = root.path().join("volume-a");
    let fresh = root.path().join("volume-b");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&fresh).unwrap();

    let control = OverlayControl::acquire_with_control_parent(&data, KEY, Some(&first)).unwrap();
    control.ensure_data_root_for_first_boot().unwrap();
    drop(control);

    let lost = OverlayControl::acquire_with_control_parent(&data, KEY, Some(&fresh)).unwrap();
    assert!(matches!(
        lost.ensure_data_root_for_first_boot(),
        Err(ControlError::MissingContinuity)
    ));
}

/// The override is canonicalised: a symlink to a real directory is accepted and
/// resolves to that directory, so the lock and ledger live at the real path.
#[test]
fn a_symlinked_override_resolves_to_its_real_directory() {
    let (root, data) = temp_data_root();
    let real = root.path().join("real-volume");
    fs::create_dir_all(&real).unwrap();
    let link = root.path().join("link-volume");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let control = OverlayControl::acquire_with_control_parent(&data, KEY, Some(&link)).unwrap();
    control.ensure_data_root_for_first_boot().unwrap();
    assert!(control
        .control_root()
        .starts_with(fs::canonicalize(&real).unwrap()));
    assert!(!control.control_root().starts_with(&link));
}

/// A symlink that points into the data tree is refused, not followed.
#[test]
fn a_symlinked_override_into_the_data_root_is_refused() {
    let (root, data) = temp_data_root();
    let inside = data.join("sub");
    fs::create_dir_all(&inside).unwrap();
    let link = root.path().join("sneaky");
    std::os::unix::fs::symlink(&inside, &link).unwrap();

    assert!(matches!(
        OverlayControl::acquire_with_control_parent(&data, KEY, Some(&link)),
        Err(ControlError::UnsafeControlPath(_))
    ));
}
