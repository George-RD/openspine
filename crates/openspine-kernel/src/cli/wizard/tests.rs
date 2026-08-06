//! Tests for `cli::wizard`, extracted from the inline `mod tests` so the
//! source file stays under the 500-line budget.

use super::*;

/// `openspine chat` holds the data-root lock for as long as it runs, and its
/// own first-run notice tells the owner to run `openspine setup`. Refusing
/// outright would make following that advice from a second terminal fail
/// instead of explaining anything.
#[test]
fn a_held_lock_degrades_to_a_reportable_vault_rather_than_an_error() {
    let dir = std::env::temp_dir().join(format!("openspine-locked-{}", ulid::Ulid::new()));
    let data_dir = dir.join("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    let config_path = config_with_data_dir(&dir);
    let key = "aa11bb22cc33dd44ee55ff6600778899aa11bb22cc33dd44ee55ff6600778899";
    std::env::set_var("OPENSPINE_ARTIFACT_KEY", key);
    let held = overlay_export_restore::acquire(&data_dir, &[0xaa; 32]).expect("hold the lock");

    let vault = open_vault(&config_path).expect("must not error");

    assert!(matches!(vault, Vault::Locked));
    assert!(vault.store().is_none());
    assert!(!vault.writable());
    assert!(
        vault.write_refusal().contains("another OpenSpine instance"),
        "{}",
        vault.write_refusal()
    );
    drop(held);
}

/// Why [`refresh_vault`] refuses to reopen an open vault: the data-root
/// lock is exclusive even against the process already holding it, so a
/// reopen after every login would fail the whole wizard.
#[test]
fn the_data_root_lock_cannot_be_acquired_twice() {
    let dir = std::env::temp_dir().join(format!("openspine-lock-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();

    let held = overlay_export_restore::acquire(&dir, &[3u8; 32]).expect("first acquire");
    let second = overlay_export_restore::acquire(&dir, &[3u8; 32]);

    assert!(second.is_err(), "the lock must be exclusive");
    drop(held);
}

/// The `/login` chat handoff depends on this: once the chat runtime's
/// `Arc` clones drop, a fresh acquisition must succeed so
/// `run_provider_login` behaves exactly like a shell invocation.
#[test]
fn login_handoff_can_reacquire_the_lock_after_chat_owners_drop() {
    let dir = std::env::temp_dir().join(format!("openspine-handoff-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();

    let held = std::sync::Arc::new(
        overlay_export_restore::acquire(&dir, &[7u8; 32]).expect("first acquire"),
    );
    let state_clone = held.clone();
    assert!(
        overlay_export_restore::acquire(&dir, &[7u8; 32]).is_err(),
        "the lock must stay exclusive while any owner lives"
    );

    drop(state_clone);
    drop(held);

    let reacquired = overlay_export_restore::acquire(&dir, &[7u8; 32]);
    assert!(
        reacquired.is_ok(),
        "dropping every owner must release the lifetime lock: {:?}",
        reacquired.err()
    );
}

fn config_with_data_dir(dir: &Path) -> PathBuf {
    let config_path = dir.join("openspine.yaml");
    std::fs::write(
        &config_path,
        format!(
            "data_dir: {}\nsandbox:\n  driver: process\nowner:\n  telegram_user_id: 1\n  \
             display_name: o\nspend_cap: {{}}\n",
            dir.join("data").display()
        ),
    )
    .unwrap();
    config_path
}

/// A fresh data root may be keyed freely.
#[test]
fn a_new_data_root_reports_no_encrypted_state() {
    let dir = std::env::temp_dir().join(format!("openspine-fresh-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(dir.join("data")).unwrap();
    let config_path = config_with_data_dir(&dir);

    assert_eq!(encrypted_state_in(&config_path), None);
}

/// An empty vault directory holds nothing to orphan, so it must not block
/// key generation.
#[test]
fn an_empty_vault_directory_reports_no_encrypted_state() {
    let dir = std::env::temp_dir().join(format!("openspine-empty-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(dir.join("data").join("credentials")).unwrap();
    let config_path = config_with_data_dir(&dir);

    assert_eq!(encrypted_state_in(&config_path), None);
}

/// Minting a fresh artifact key over a populated data root does not fail
/// loudly: it silently makes every stored credential undecryptable.
#[test]
fn a_populated_data_root_is_detected_so_no_new_artifact_key_is_minted() {
    let dir = std::env::temp_dir().join(format!("openspine-keyed-{}", ulid::Ulid::new()));
    let credentials = dir.join("data").join("credentials");
    std::fs::create_dir_all(&credentials).unwrap();
    std::fs::write(credentials.join("provider.anthropic.access_token"), b"x").unwrap();
    let config_path = config_with_data_dir(&dir);

    assert_eq!(encrypted_state_in(&config_path), Some(credentials));
}

/// `kernel.db` is a file, not a directory: existence alone is the signal.
#[test]
fn an_existing_kernel_database_is_detected() {
    let dir = std::env::temp_dir().join(format!("openspine-db-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::write(dir.join("data").join("kernel.db"), b"sqlite").unwrap();
    let config_path = config_with_data_dir(&dir);

    assert_eq!(
        encrypted_state_in(&config_path),
        Some(dir.join("data").join("kernel.db"))
    );
}

#[test]
fn refresh_leaves_an_unavailable_vault_unavailable_without_a_configuration() {
    let mut vault = Vault::Unavailable;

    refresh_vault(&mut vault, Path::new("/nonexistent/openspine.yaml")).unwrap();

    assert!(vault.store().is_none());
}
