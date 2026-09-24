use super::*;
use std::fs;
use std::os::unix::fs::symlink;

#[test]
fn overlay_review_refuses_a_linked_family_instead_of_assessing_empty_state() {
    let root = tempfile::tempdir().unwrap();
    let overlay = root.path().join("artifacts.d");
    let elsewhere = root.path().join("elsewhere");
    fs::create_dir(&overlay).unwrap();
    fs::create_dir(&elsewhere).unwrap();
    symlink(&elsewhere, overlay.join("routes")).unwrap();
    assert!(capture(&overlay, &HashMap::new()).is_err());
}
