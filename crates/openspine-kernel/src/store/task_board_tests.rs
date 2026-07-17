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
fn task(store: &Store, status: TaskStatus, due_at: Option<Timestamp>) -> Task {
    let row = Task {
        schema_version: 1,
        id: Ulid::new(),
        owner_principal_id: Ulid::new(),
        status,
        owning_worker: Some(openspine_schemas::task::WorkerId::new("worker").unwrap()),
        owning_grant_id: None,
        due_at,
        reminder_at: None,
        due_timer_id: None,
        reminder_timer_id: None,
        dependencies: vec![Ulid::new()],
        provenance: TaskProvenance::AskedAbout {
            reference: ref_of('a'),
            asked_at: Timestamp::from_second(1).unwrap(),
        },
        title_ref: ref_of('b'),
        created_at: Timestamp::from_second(1).unwrap(),
    };
    store.insert_task(&row).unwrap();
    row
}
fn task_with_owner(
    store: &Store,
    status: TaskStatus,
    due_at: Option<Timestamp>,
    owner: Ulid,
) -> Task {
    let row = Task {
        schema_version: 1,
        id: Ulid::new(),
        owner_principal_id: owner,
        status,
        owning_worker: Some(openspine_schemas::task::WorkerId::new("worker").unwrap()),
        owning_grant_id: None,
        due_at,
        reminder_at: None,
        due_timer_id: None,
        reminder_timer_id: None,
        dependencies: vec![Ulid::new()],
        provenance: TaskProvenance::AskedAbout {
            reference: ref_of('a'),
            asked_at: Timestamp::from_second(1).unwrap(),
        },
        title_ref: ref_of('b'),
        created_at: Timestamp::from_second(1).unwrap(),
    };
    store.insert_task(&row).unwrap();
    row
}
#[test]
fn task_lifecycle_persists_grant_dependencies_and_both_timers() {
    let store = Store::open_in_memory().unwrap();
    let due_at = Timestamp::from_second(10).unwrap();
    let reminder_at = Timestamp::from_second(20).unwrap();
    let row = Task {
        schema_version: 1,
        id: Ulid::new(),
        owner_principal_id: Ulid::new(),
        status: TaskStatus::Blocked,
        owning_worker: Some(WorkerId::new("worker").unwrap()),
        owning_grant_id: Some(Ulid::new()),
        due_at: Some(due_at),
        reminder_at: Some(reminder_at),
        due_timer_id: None,
        reminder_timer_id: None,
        dependencies: vec![Ulid::new()],
        provenance: TaskProvenance::AskedAbout {
            reference: ref_of('a'),
            asked_at: Timestamp::from_second(1).unwrap(),
        },
        title_ref: ref_of('b'),
        created_at: Timestamp::from_second(1).unwrap(),
    };
    store.insert_task(&row).unwrap();
    let due_timer = Ulid::new().to_string();
    let reminder_timer = Ulid::new().to_string();
    store
        .schedule_task_timer(
            &due_timer,
            &row.id.to_string(),
            TaskTimerKind::Deadline,
            due_at,
        )
        .unwrap();
    store
        .schedule_task_timer(
            &reminder_timer,
            &row.id.to_string(),
            TaskTimerKind::Reminder,
            reminder_at,
        )
        .unwrap();
    let back = store.get_task(row.id).unwrap().unwrap();
    assert_eq!(back.status, TaskStatus::Blocked);
    assert_eq!(back.owning_grant_id, row.owning_grant_id);
    assert_eq!(back.due_at, row.due_at);
    assert_eq!(back.reminder_at, row.reminder_at);
    assert_eq!(back.due_timer_id, Some(due_timer));
    assert_eq!(back.reminder_timer_id, Some(reminder_timer));
    assert_eq!(back.dependencies, row.dependencies);
    assert_eq!(back.provenance, row.provenance);
}
#[test]
fn task_status_and_full_json_round_trip() {
    let store = Store::open_in_memory().unwrap();
    let row = task(
        &store,
        TaskStatus::Blocked,
        Some(Timestamp::from_second(10).unwrap()),
    );
    let back = store.get_task(row.id).unwrap().unwrap();
    assert_eq!(back.status, TaskStatus::Blocked);
    assert_eq!(back.dependencies, row.dependencies);
    assert_eq!(back.provenance, row.provenance);
}
#[test]
fn master_slice_is_bounded_and_excludes_task_detail() {
    let store = Store::open_in_memory().unwrap();
    let a = task(
        &store,
        TaskStatus::Open,
        Some(Timestamp::from_second(10).unwrap()),
    );
    task(&store, TaskStatus::Blocked, None);
    let slice = store
        .master_slice(Timestamp::from_second(20).unwrap(), a.owner_principal_id, 1)
        .unwrap();
    assert_eq!(slice.len(), 1);
    let json = serde_json::to_string(&slice).unwrap();
    assert!(!json.contains("dependencies"));
    assert!(!json.contains("provenance"));
    assert!(!json.contains("owning_grant_id"));
}
#[test]
fn anchored_slice_includes_not_yet_due_focal_task_and_honors_limit_one() {
    let store = Store::open_in_memory().unwrap();
    let focal = task(
        &store,
        TaskStatus::Open,
        Some(Timestamp::from_second(100).unwrap()),
    );
    let slice = store
        .master_slice_for_task(focal.id, Timestamp::from_second(20).unwrap(), 1)
        .unwrap();
    assert_eq!(slice.len(), 1);
    assert_eq!(slice[0].id, focal.id);
    assert_eq!(slice[0].due_at, focal.due_at);
}
#[test]
fn master_slice_is_owner_scoped() {
    let store = Store::open_in_memory().unwrap();
    let a = task(
        &store,
        TaskStatus::Open,
        Some(Timestamp::from_second(10).unwrap()),
    );
    task(
        &store,
        TaskStatus::Open,
        Some(Timestamp::from_second(10).unwrap()),
    );
    let slice = store
        .master_slice(
            Timestamp::from_second(20).unwrap(),
            a.owner_principal_id,
            10,
        )
        .unwrap();
    assert_eq!(slice.len(), 1);
    assert_eq!(slice[0].id, a.id);
}
#[test]
fn all_slice_categories_are_owner_scoped() {
    let store = Store::open_in_memory().unwrap();
    let owner_a = Ulid::new();
    let owner_b = Ulid::new();
    let ta_open = task_with_owner(
        &store,
        TaskStatus::Open,
        Some(Timestamp::from_second(10).unwrap()),
        owner_a,
    );
    let ta_blocked = task_with_owner(&store, TaskStatus::Blocked, None, owner_a);
    let tb_open = task_with_owner(
        &store,
        TaskStatus::Open,
        Some(Timestamp::from_second(10).unwrap()),
        owner_b,
    );
    let tb_blocked = task_with_owner(&store, TaskStatus::Blocked, None, owner_b);
    let owner_a_ids: std::collections::HashSet<Ulid> =
        [ta_open.id, ta_blocked.id].into_iter().collect();
    let owner_b_ids: std::collections::HashSet<Ulid> =
        [tb_open.id, tb_blocked.id].into_iter().collect();
    let due = store
        .tasks_due_now(Timestamp::from_second(20).unwrap(), owner_a, 10)
        .unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].id, ta_open.id);
    let blocked = store.blocked_tasks(owner_a, 10).unwrap();
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0].id, ta_blocked.id);
    let asked = store.asked_about_tasks(owner_a, 10).unwrap();
    let asked_ids: std::collections::HashSet<Ulid> = asked.iter().map(|s| s.id).collect();
    assert!(asked_ids.is_subset(&owner_a_ids));
    assert!(asked_ids.is_disjoint(&owner_b_ids));
    assert!(asked_ids.contains(&ta_open.id));
}
#[test]
fn slice_queries_break_ties_by_id_deterministically() {
    let store = Store::open_in_memory().unwrap();
    let owner = Ulid::new();
    let t1 = task_with_owner(
        &store,
        TaskStatus::Open,
        Some(Timestamp::from_second(10).unwrap()),
        owner,
    );
    let t2 = task_with_owner(
        &store,
        TaskStatus::Open,
        Some(Timestamp::from_second(10).unwrap()),
        owner,
    );
    let slice_limit_1 = store
        .tasks_due_now(Timestamp::from_second(20).unwrap(), owner, 1)
        .unwrap();
    assert_eq!(slice_limit_1.len(), 1);
    let expected_first_id = if t1.id < t2.id { t1.id } else { t2.id };
    assert_eq!(slice_limit_1[0].id, expected_first_id);
}
#[test]
fn task_timer_scheduling_is_atomic_and_fires_once() {
    let store = Store::open_in_memory().unwrap();
    let row = task(
        &store,
        TaskStatus::Open,
        Some(Timestamp::from_second(10).unwrap()),
    );
    let timer_id = row.id.to_string();
    store
        .schedule_task_timer(
            &timer_id,
            &row.id.to_string(),
            TaskTimerKind::Deadline,
            Timestamp::from_second(10).unwrap(),
        )
        .unwrap();
    let linked = store.task_by_timer_id(&timer_id).unwrap().unwrap();
    assert_eq!(linked.id, row.id);
    assert_eq!(linked.due_timer_id.as_deref(), Some(timer_id.as_str()));
    let scheduled_before = store
        .count_audit_events_of_kind("workflow.timer_scheduled")
        .unwrap();
    let links_before: i64 = {
        let conn = store.conn.lock();
        conn.query_row("SELECT COUNT(*) FROM task_timer_links", [], |r| r.get(0))
            .unwrap()
    };
    let dup = store.schedule_task_timer(
        &timer_id,
        &row.id.to_string(),
        TaskTimerKind::Deadline,
        Timestamp::from_second(10).unwrap(),
    );
    assert!(dup.is_err());
    assert_eq!(
        store
            .count_audit_events_of_kind("workflow.timer_scheduled")
            .unwrap(),
        scheduled_before,
        "conflicting schedule must not commit an audit row"
    );
    let links_after: i64 = {
        let conn = store.conn.lock();
        conn.query_row("SELECT COUNT(*) FROM task_timer_links", [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(
        links_after, links_before,
        "conflicting schedule must not add a link"
    );
    assert_eq!(
        store
            .fire_due_timers(Timestamp::from_second(10).unwrap())
            .unwrap()
            .len(),
        1
    );
    assert!(store
        .fire_due_timers(Timestamp::from_second(11).unwrap())
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .count_audit_events_of_kind("workflow.timer_fired")
            .unwrap(),
        1
    );
}
#[test]
fn schedule_rolls_back_after_timer_insert_failure() {
    let store = Store::open_in_memory().unwrap();
    let row = task(
        &store,
        TaskStatus::Open,
        Some(Timestamp::from_second(10).unwrap()),
    );
    let timer_id = row.id.to_string();
    {
        let conn = store.conn.lock();
        conn.execute(
            "INSERT INTO workflow_timers (timer_id, run_id, fires_at, status, fired_event_id) \
             VALUES (?1, ?2, ?3, 'pending', NULL)",
            params![
                timer_id,
                "pre-existing",
                timestamp_to_epoch_nanos(Timestamp::from_second(10).unwrap()).unwrap()
            ],
        )
        .unwrap();
    }
    let scheduled_before = store
        .count_audit_events_of_kind("workflow.timer_scheduled")
        .unwrap();
    let err = store.schedule_task_timer(
        &timer_id,
        &row.id.to_string(),
        TaskTimerKind::Deadline,
        Timestamp::from_second(10).unwrap(),
    );
    assert!(err.is_err());
    let back = store.get_task(row.id).unwrap().unwrap();
    assert_eq!(
        back.due_timer_id, None,
        "task_json timer field must roll back"
    );
    assert!(
        store.task_by_timer_id(&timer_id).unwrap().is_none(),
        "task_timer_links row must roll back"
    );
    assert_eq!(
        store
            .count_audit_events_of_kind("workflow.timer_scheduled")
            .unwrap(),
        scheduled_before,
        "scheduled audit row must roll back"
    );
}
#[test]
fn scheduling_rejects_mismatched_or_terminal_task() {
    let store = Store::open_in_memory().unwrap();
    let open = task(
        &store,
        TaskStatus::Open,
        Some(Timestamp::from_second(10).unwrap()),
    );
    assert!(store
        .schedule_task_timer(
            &Ulid::new().to_string(),
            &open.id.to_string(),
            TaskTimerKind::Deadline,
            Timestamp::from_second(99).unwrap(),
        )
        .is_err());
    let done = task(&store, TaskStatus::Done, None);
    assert!(store
        .schedule_task_timer(
            &Ulid::new().to_string(),
            &done.id.to_string(),
            TaskTimerKind::Reminder,
            Timestamp::from_second(10).unwrap(),
        )
        .is_err());
}
#[test]
fn task_timer_link_is_globally_unique() {
    let store = Store::open_in_memory().unwrap();
    let a = task(
        &store,
        TaskStatus::Open,
        Some(Timestamp::from_second(10).unwrap()),
    );
    let b = task(
        &store,
        TaskStatus::Open,
        Some(Timestamp::from_second(10).unwrap()),
    );
    let shared = Ulid::new().to_string();
    assert!(store
        .schedule_task_timer(
            &shared,
            &a.id.to_string(),
            TaskTimerKind::Deadline,
            Timestamp::from_second(10).unwrap(),
        )
        .is_ok());
    assert!(store
        .schedule_task_timer(
            &shared,
            &b.id.to_string(),
            TaskTimerKind::Deadline,
            Timestamp::from_second(10).unwrap(),
        )
        .is_err());
}
#[test]
fn insert_task_rejects_prepopulated_timer_ids() {
    let store = Store::open_in_memory().unwrap();
    let mut t = task(
        &store,
        TaskStatus::Open,
        Some(Timestamp::from_second(10).unwrap()),
    );
    t.due_timer_id = Some("pre".to_string());
    assert!(store.insert_task(&t).is_err());
}
#[test]
fn dispatch_state_terminal_is_durable_and_idempotent() {
    let store = Store::open_in_memory().unwrap();
    let event_id = Ulid::new().to_string();
    let timer_id = Ulid::new().to_string();
    store
        .mark_dispatch_terminal(&event_id, &timer_id, None, "explicit_skip", &event_id)
        .unwrap();
    let row = store.dispatch_state_for_key(&event_id).unwrap().unwrap();
    assert_eq!(row.state, TimerDispatchState::Terminal);
    assert_eq!(row.terminal_reason.as_deref(), Some("explicit_skip"));
    assert!(store.timer_event_already_processed(&event_id).unwrap());
    store
        .mark_dispatch_terminal(&event_id, &timer_id, None, "explicit_skip", &event_id)
        .unwrap();
    assert_eq!(
        store
            .dispatch_state_for_key(&event_id)
            .unwrap()
            .unwrap()
            .state,
        TimerDispatchState::Terminal
    );
}
