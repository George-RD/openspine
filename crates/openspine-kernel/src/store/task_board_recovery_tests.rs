use super::*;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::digest::Digest;
use openspine_schemas::task::{Task, TaskProvenance, TaskStatus, WorkerId};

fn ref_of(byte: char) -> ArtifactRef {
    ArtifactRef {
        digest: Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).unwrap(),
        schema_version: 1,
    }
}

#[test]
fn dependency_wake_requires_all_dependencies_and_unblocks_task() {
    let store = Store::open_in_memory().unwrap();
    let owner = Ulid::new();
    let dep_a = Ulid::new();
    let dep_b = Ulid::new();
    let dependent = Ulid::new();
    store
        .insert_raw_principal_for_test(&openspine_schemas::principal::Principal {
            id: owner,
            identity_id: Ulid::new(),
            is_owner: false,
            schema_version: 1,
        })
        .unwrap();
    let make = |id, status, dependencies| Task {
        schema_version: 1,
        id,
        owner_principal_id: owner,
        status,
        owning_worker: Some(WorkerId::new("worker").unwrap()),
        owning_grant_id: None,
        due_at: None,
        reminder_at: None,
        due_timer_id: None,
        reminder_timer_id: None,
        dependencies,
        provenance: TaskProvenance::AskedAbout {
            reference: ref_of('a'),
            asked_at: Timestamp::from_second(1).unwrap(),
        },
        title_ref: ref_of('b'),
        created_at: Timestamp::from_second(1).unwrap(),
    };
    store
        .insert_task(&make(dep_a, TaskStatus::Open, vec![]))
        .unwrap();
    store
        .insert_task(&make(dep_b, TaskStatus::Open, vec![]))
        .unwrap();
    store
        .insert_task(&make(dependent, TaskStatus::Blocked, vec![dep_a, dep_b]))
        .unwrap();
    let timer_id = Ulid::new().to_string();
    let event_id = Ulid::new().to_string();
    store
        .insert_dependency_waiter(dependent, owner, dep_a, &timer_id, &event_id)
        .unwrap();
    store
        .insert_dependency_waiter(dependent, owner, dep_b, &timer_id, &event_id)
        .unwrap();
    assert!(store.mark_task_done_and_poll(dep_a).unwrap().is_empty());
    assert_eq!(
        store.get_task(dependent).unwrap().unwrap().status,
        TaskStatus::Blocked
    );
    let wakes = store.mark_task_done_and_poll(dep_b).unwrap();
    assert_eq!(wakes.len(), 1);
    assert_eq!(
        store.get_task(dependent).unwrap().unwrap().status,
        TaskStatus::Open
    );
    assert_eq!(store.take_ready_wakes().unwrap().len(), 1);
    store
        .consume_dependency_waiter(dependent, &timer_id)
        .unwrap();
    assert!(store.take_ready_wakes().unwrap().is_empty());
}
