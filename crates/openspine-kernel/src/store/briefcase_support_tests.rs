//! Atomic grant + briefcase persistence tests (kept here to stay under the
//! 500-line gate on `store/tests.rs`). These exercise
//! [`super::super::Store::insert_grant_and_briefcase_atomic`] directly.

#![allow(clippy::too_many_lines)]

use super::tests::sample_grant;
use super::Store;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::briefcase::{
    Briefcase, CounterpartyRef, RelationshipTier, TaskClass, TaskShape,
};
use openspine_schemas::digest::Digest;

fn minimal_briefcase() -> Briefcase {
    Briefcase {
        schema_version: 1,
        task_shape: TaskShape {
            route_id: "owner_telegram_main_assistant".to_string(),
            workflow_id: "owner_control_conversation".to_string(),
            counterparty: CounterpartyRef::Unresolved {
                channel: "email".to_string(),
                identifier: "thread-1".to_string(),
            },
        },
        source_snapshot_id: Digest::parse(format!("sha256:{}", "0".repeat(64))).unwrap(),
        depth: 1,
        tier: RelationshipTier::Stranger,
        class: TaskClass::Conversation,
        sections: vec![],
        top_up_log: vec![],
    }
}

#[test]
fn atomic_grant_and_briefcase_persists_both_on_success() {
    let store = Store::open_in_memory().unwrap();
    let grant = sample_grant("atomic-ok-token");
    let pending_message_ref = ArtifactRef {
        digest: Digest::parse(format!("sha256:{}", "c".repeat(64))).unwrap(),
        schema_version: 1,
    };
    store
        .insert_grant_and_briefcase_atomic(&grant, &pending_message_ref, 555, &minimal_briefcase())
        .unwrap();
    // Both rows landed and are mutually consistent.
    assert!(store
        .find_task_grant_by_token("atomic-ok-token")
        .unwrap()
        .is_some());
    assert!(store.find_briefcase(grant.id).unwrap().is_some());
}

#[test]
fn atomic_grant_and_briefcase_rolls_back_on_briefcase_failure() {
    // Crash-window proof (D-050): if the briefcase write fails, the already
    // queued grant insert must roll back too — no orphan grant, no partial
    // briefcase. We force the briefcase INSERT to ABORT with a temporary
    // BEFORE INSERT trigger, then assert neither row exists.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("atomic.db");
    let store = Store::open(&path).unwrap();
    let grant = sample_grant("atomic-fail-token");
    let pending_message_ref = ArtifactRef {
        digest: Digest::parse(format!("sha256:{}", "c".repeat(64))).unwrap(),
        schema_version: 1,
    };
    {
        let conn = store.conn.lock();
        conn.execute_batch(
            "CREATE TRIGGER abort_briefcase BEFORE INSERT ON briefcases BEGIN \
             SELECT RAISE(ABORT, 'injected briefcase write failure'); END;",
        )
        .unwrap();
    }
    let result = store.insert_grant_and_briefcase_atomic(
        &grant,
        &pending_message_ref,
        555,
        &minimal_briefcase(),
    );
    assert!(
        result.is_err(),
        "briefcase write failure must abort the transaction"
    );
    // Re-open to clear the in-memory trigger state and inspect durable rows.
    drop(store);
    let store = Store::open(&path).unwrap();
    assert!(
        store
            .find_task_grant_by_token("atomic-fail-token")
            .unwrap()
            .is_none(),
        "orphan grant must not persist after briefcase write fails"
    );
    assert!(
        store.find_briefcase(grant.id).unwrap().is_none(),
        "briefcase must not persist after rollback"
    );
}
