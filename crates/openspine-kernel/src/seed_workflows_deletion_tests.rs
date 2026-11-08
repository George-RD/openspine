use super::*;

#[test]
fn owner_deleted_seed_retires_once_without_false_dangling_audits() {
    let f = Fixture::new();
    f.accept();
    let id = "research_and_brief_seed";
    std::fs::remove_file(f.path(id)).unwrap();
    for _ in 0..2 {
        assert!(!f.boot().registry.workflows.contains_key(id));
    }
    assert_eq!(
        f.store
            .count_audit_events_of_kind("artifact.owner_accepted_rejected")
            .unwrap(),
        0
    );
    let proposal = f
        .store
        .find_proposed_artifact("workflow", id, 1)
        .unwrap()
        .unwrap();
    assert_eq!(proposal.state, Lifecycle::Retired);
    assert_eq!(f.accept(), 0, "re-init never reactivates a retired seed");
    let yaml = crate::seed_workflows::all()
        .iter()
        .find(|(seed, _)| *seed == id)
        .unwrap()
        .1;
    std::fs::write(f.path(id), yaml).unwrap();
    assert!(
        !f.boot().registry.workflows.contains_key(id),
        "same-version file reappearance cannot undo terminal retirement"
    );
    assert_eq!(
        f.store.highest_active_version("workflow", id).unwrap(),
        None
    );
    let events: Vec<serde_json::Value> = f
        .store
        .all_audit_event_jsons()
        .unwrap()
        .iter()
        .map(|json| serde_json::from_str(json).unwrap())
        .collect();
    let retired: Vec<_> = events
        .iter()
        .filter(|event| event["kind"] == "artifact.retired")
        .collect();
    assert_eq!(retired.len(), 1);
    assert!(retired[0]["reason"].as_str().unwrap().contains("deleted"));
    assert!(!events.iter().any(|event| event["reason"]
        .as_str()
        .is_some_and(|reason| reason.contains("typed references now dangling"))));
    assert!(f.store.verify_audit_chain().unwrap());
}

#[test]
fn deletion_retirement_and_receipt_roll_back_together_on_audit_failure() {
    let f = Fixture::new();
    f.accept();
    let id = "research_and_brief_seed";
    std::fs::remove_file(f.path(id)).unwrap();
    f.store.with_conn_for_test(|conn| {
        conn.execute_batch(
            "CREATE TRIGGER reject_seed_retirement BEFORE INSERT ON audit_log
         WHEN NEW.kind = 'artifact.retired'
         BEGIN SELECT RAISE(ABORT, 'injected retirement audit failure'); END;",
        )
        .unwrap()
    });
    assert!(
        crate::overlay_startup::load(&Fixture::lyra(), f.dir.path(), &f.store, &f.artifacts)
            .is_err()
    );
    assert_eq!(
        f.store
            .find_proposed_artifact("workflow", id, 1)
            .unwrap()
            .unwrap()
            .state,
        Lifecycle::Active
    );
    assert_eq!(
        f.store
            .count_audit_events_of_kind("artifact.retired")
            .unwrap(),
        0
    );
    f.store.with_conn_for_test(|conn| {
        conn.execute_batch("DROP TRIGGER reject_seed_retirement")
            .unwrap()
    });
    f.boot();
    f.boot();
    assert_eq!(
        f.store
            .find_proposed_artifact("workflow", id, 1)
            .unwrap()
            .unwrap()
            .state,
        Lifecycle::Retired
    );
    assert_eq!(
        f.store
            .count_audit_events_of_kind("artifact.retired")
            .unwrap(),
        1
    );
    assert_eq!(
        f.store
            .count_audit_events_of_kind("artifact.owner_accepted_rejected")
            .unwrap(),
        0
    );
    assert!(f.store.verify_audit_chain().unwrap());
}

#[test]
fn relocated_data_root_never_reads_as_owner_deletion() {
    let f = Fixture::new();
    assert_eq!(f.accept(), 4);
    // The recorded source paths point at the old root; the seeds now live
    // under a different data root with identical bytes.
    let moved = tempfile::tempdir().unwrap();
    let moved_workflows = moved.path().join("artifacts.d").join("workflows");
    std::fs::create_dir_all(&moved_workflows).unwrap();
    for entry in std::fs::read_dir(f.overlay().join("workflows")).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), moved_workflows.join(entry.file_name())).unwrap();
    }
    std::fs::remove_dir_all(f.overlay()).unwrap();
    let startup =
        crate::overlay_startup::load(&Fixture::lyra(), moved.path(), &f.store, &f.artifacts)
            .unwrap();
    for (id, _) in crate::seed_workflows::all() {
        assert_eq!(
            f.store
                .find_proposed_artifact("workflow", id, 1)
                .unwrap()
                .unwrap()
                .state,
            Lifecycle::Active,
            "{id} must not be retired after relocation"
        );
    }
    assert!(startup
        .registry
        .workflows
        .contains_key("research_and_brief_seed"));
    assert_eq!(
        f.store
            .count_audit_events_of_kind("artifact.retired")
            .unwrap(),
        0
    );
}

#[test]
fn dangling_symlink_in_place_of_a_seed_is_not_a_deletion() {
    let f = Fixture::new();
    f.accept();
    let id = "research_and_brief_seed";
    std::fs::remove_file(f.path(id)).unwrap();
    std::os::unix::fs::symlink(f.dir.path().join("missing-target.yaml"), f.path(id)).unwrap();
    let _ = crate::overlay_startup::load(&Fixture::lyra(), f.dir.path(), &f.store, &f.artifacts);
    assert_eq!(
        f.store
            .find_proposed_artifact("workflow", id, 1)
            .unwrap()
            .unwrap()
            .state,
        Lifecycle::Active
    );
    assert_eq!(
        f.store
            .count_audit_events_of_kind("artifact.retired")
            .unwrap(),
        0
    );
}
