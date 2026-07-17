use super::{BootClockCheck, Store};
use jiff::Timestamp;
use tempfile::tempdir;

#[test]
fn test_clock_regression_detection() {
    let dir = tempdir().unwrap();
    let store = Store::open(&dir.path().join("kernel.db")).unwrap();
    let now_ms = 1_000_000_000;
    assert_eq!(
        store.check_boot_clock(now_ms).unwrap(),
        BootClockCheck::Ok {
            high_water_ms: now_ms
        }
    );
    let regressed_ms = now_ms - 61_000;
    assert_eq!(
        store.check_boot_clock(regressed_ms).unwrap(),
        BootClockCheck::Regressed {
            high_water_ms: now_ms,
            now_ms: regressed_ms
        }
    );
    assert_eq!(
        store.check_boot_clock(now_ms).unwrap(),
        BootClockCheck::Ok {
            high_water_ms: now_ms
        }
    );
    assert_eq!(
        store.check_boot_clock(now_ms - 30_000).unwrap(),
        BootClockCheck::Ok {
            high_water_ms: now_ms
        }
    );
    assert_eq!(
        store.check_boot_clock(now_ms + 10_000).unwrap(),
        BootClockCheck::Ok {
            high_water_ms: now_ms + 10_000
        }
    );
}

#[test]
fn test_clock_regression_saturating_underflow() {
    let dir = tempdir().unwrap();
    let store = Store::open(&dir.path().join("kernel.db")).unwrap();
    assert_eq!(
        store.check_boot_clock(i64::MIN).unwrap(),
        BootClockCheck::Ok {
            high_water_ms: i64::MIN
        }
    );
    assert_eq!(
        store.check_boot_clock(10).unwrap(),
        BootClockCheck::Ok { high_water_ms: 10 }
    );
    assert_eq!(
        store.check_boot_clock(i64::MIN).unwrap(),
        BootClockCheck::Regressed {
            high_water_ms: 10,
            now_ms: i64::MIN
        }
    );
}

#[test]
fn runtime_timer_driver_observation_is_durable() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("kernel.db");
    let observed_ms = 9_000_000_000_i64;
    let store = Store::open(&path).unwrap();
    crate::workflow::run_timer_driver_iteration(
        &store,
        Timestamp::from_second(observed_ms / 1_000).unwrap(),
    )
    .unwrap();
    drop(store);
    let reopened = Store::open(&path).unwrap();
    assert_eq!(
        reopened.validate_boot_clock(observed_ms - 61_000).unwrap(),
        BootClockCheck::Regressed {
            high_water_ms: observed_ms,
            now_ms: observed_ms - 61_000
        }
    );
}

#[test]
fn post_bind_startup_failure_leaves_candidate_uncommitted() {
    let store = Store::open_in_memory().unwrap();
    let candidate = 42_000_i64;
    let error = crate::commit_post_bind_clock(&store, candidate, || candidate - 61_000)
        .expect_err("a regressed post-bind sample must refuse startup");
    assert!(error
        .to_string()
        .contains("wall clock regressed during startup"));
    assert_eq!(
        store.validate_boot_clock(candidate - 61_000).unwrap(),
        BootClockCheck::Ok {
            high_water_ms: candidate - 61_000
        }
    );
}

#[test]
fn runtime_clock_observation_survives_restart_and_preserves_maximum() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("kernel.db");
    let observed = 9_000_000_000_i64;
    let store = Store::open(&path).unwrap();
    assert_eq!(store.observe_runtime_clock(observed).unwrap(), observed);
    drop(store);
    let reopened = Store::open(&path).unwrap();
    assert_eq!(
        reopened.check_boot_clock(observed - 61_000).unwrap(),
        BootClockCheck::Regressed {
            high_water_ms: observed,
            now_ms: observed - 61_000
        }
    );
    assert_eq!(
        reopened.observe_runtime_clock(observed - 1).unwrap(),
        observed
    );
}

#[test]
fn startup_clock_validation_does_not_persist_until_commit() {
    let store = Store::open_in_memory().unwrap();
    let candidate = 42_000_i64;
    assert_eq!(
        store.validate_boot_clock(candidate).unwrap(),
        BootClockCheck::Ok {
            high_water_ms: candidate
        }
    );
    assert_eq!(
        store.validate_boot_clock(candidate - 61_000).unwrap(),
        BootClockCheck::Ok {
            high_water_ms: candidate - 61_000
        }
    );
    assert_eq!(
        store.commit_boot_clock(candidate).unwrap(),
        BootClockCheck::Ok {
            high_water_ms: candidate
        }
    );
    assert_eq!(
        store.validate_boot_clock(candidate - 61_000).unwrap(),
        BootClockCheck::Regressed {
            high_water_ms: candidate,
            now_ms: candidate - 61_000
        }
    );
}

#[test]
fn concurrent_clock_updates_preserve_the_durable_maximum() {
    use std::sync::{Arc, Barrier};
    use std::thread;
    let dir = tempdir().unwrap();
    let path = dir.path().join("kernel.db");
    let first = Store::open(&path).unwrap();
    let second = Store::open(&path).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let low_barrier = barrier.clone();
    let low = thread::spawn(move || {
        low_barrier.wait();
        first.check_boot_clock(100_000).unwrap()
    });
    let high_barrier = barrier;
    let high = thread::spawn(move || {
        high_barrier.wait();
        second.check_boot_clock(200_000).unwrap()
    });
    let _ = low.join().unwrap();
    let _ = high.join().unwrap();
    let reopened = Store::open(&path).unwrap();
    assert_eq!(
        reopened.check_boot_clock(139_000).unwrap(),
        BootClockCheck::Regressed {
            high_water_ms: 200_000,
            now_ms: 139_000
        }
    );
}
