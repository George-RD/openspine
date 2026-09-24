//! Real SQLite concurrency regression: a lookup waits behind a newer activation.
use std::cell::RefCell;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use super::*;
use crate::store::standing_rules_tests::manifest;
use openspine_schemas::standing_rule::BudgetWindow;

thread_local! {
    static WRITER_WAIT: RefCell<Option<(Sender<()>, Receiver<()>)>> = const { RefCell::new(None) };
}

// A one-shot connection-local barrier. No scheduler sleeps or production hook:
// SQLite calls this when the lookup needs the lock held by the other connection.
fn wait_for_activation(_attempt: i32) -> bool {
    WRITER_WAIT.with(|wait| {
        let Some((blocked, committed)) = wait.borrow_mut().take() else {
            return false;
        };
        blocked.send(()).is_ok() && committed.recv_timeout(Duration::from_secs(10)).is_ok()
    })
}

#[test]
fn expiry_lookup_does_not_lapse_a_new_version_committed_while_it_waits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("kernel.db");
    let writer = Store::open(&path).unwrap();
    let lookup = Store::open(&path).unwrap();
    let activated: Timestamp = "2026-08-21T10:00:00Z".parse().unwrap();
    let now = activated + Duration::from_secs(120);
    let window = BudgetWindow {
        max: 10,
        window_secs: 3600,
    };
    let first = manifest("lookup-race", "appointment.book", 60, window, window, None);
    writer
        .activate_standing_rule(&first, None, activated)
        .unwrap();
    let mut second = first.clone();
    second.version = 2;

    let (blocked_tx, blocked_rx) = channel();
    let (committed_tx, committed_rx) = channel();
    let handle = writer
        .with_immediate_tx(|tx| {
            Store::activate_standing_rule_in_tx(tx, &second, None, now)?;
            let handle = std::thread::spawn(move || {
                WRITER_WAIT.with(|wait| *wait.borrow_mut() = Some((blocked_tx, committed_rx)));
                lookup.with_conn_for_test(|conn| {
                    conn.busy_handler(Some(wait_for_activation)).unwrap()
                });
                lookup.active_standing_rule_for_action(&ActionId::new("appointment.book"), now)
            });
            blocked_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("lookup reached writer lock");
            Ok(handle)
        })
        .unwrap();
    committed_tx.send(()).unwrap();
    let observed = handle
        .join()
        .unwrap()
        .unwrap()
        .expect("new version remains active");
    assert_eq!(observed.version, 2);
    assert!(writer.standing_rule_is_current("lookup-race", 2).unwrap());
    assert!(writer.verify_audit_chain().unwrap());
}
