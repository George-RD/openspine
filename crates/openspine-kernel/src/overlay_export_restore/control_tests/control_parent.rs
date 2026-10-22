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
