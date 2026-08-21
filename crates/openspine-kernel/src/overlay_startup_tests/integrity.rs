use super::*;

#[test]
fn startup_terminal_erasure_revokes_scoped_rule_before_reuse() {
    use openspine_schemas::digest::Digest;
    use rusqlite::params;
    use serde_json::json;

    let (_root, data) = temp_data_root();
    let db_path = data.join("kernel.db");
    let _ = fs::remove_file(&db_path);
    let store = Store::open(&db_path).unwrap();
    let artifacts =
        crate::artifact_store::ArtifactStore::open(data.join("artifacts"), *KEY).unwrap();
    let ops = acquire(&data, KEY).unwrap();
    ops.initialize_terminal_ledger().unwrap();

    let counterparty = Ulid::from(11_u128);
    let mut manifest = crate::store::standing_rules_tests::manifest(
        "rule-startup-erasure",
        "email.create_draft",
        7 * 24 * 3600,
        openspine_schemas::standing_rule::BudgetWindow {
            max: 5,
            window_secs: 7 * 24 * 3600,
        },
        openspine_schemas::standing_rule::BudgetWindow {
            max: 3,
            window_secs: 3600,
        },
        None,
    );
    let zero_digest = Digest::parse(format!("sha256:{}", "0".repeat(64))).unwrap();
    let mut manifest_json = serde_json::to_value(&manifest).unwrap();
    manifest_json["reviewed_scope"] = json!({
        "scope": {
            "schema_version": 1,
            "scope_version": 1,
            "action_id": "email.create_draft",
            "descriptor_version": 1,
            "dimensions": {
                "counterparty": {
                    "kind": "counterparty",
                    "value": counterparty.to_string()
                }
            },
            "context_class_digest": zero_digest.to_string()
        },
        "reviewed_scope_digest": zero_digest.to_string(),
        "compatibility_digest": zero_digest.to_string()
    });
    manifest = serde_json::from_value(manifest_json.clone()).unwrap();

    let activated_at: i64 = Timestamp::now()
        .as_nanosecond()
        .try_into()
        .expect("test timestamp fits i64");
    store.with_conn_for_test(|conn| {
        conn.execute(
            "INSERT INTO standing_rules (
                rule_id, artifact_id, version, action_id, rule_json,
                quota_max, quota_window_secs, rate_max, rate_window_secs,
                expires_after_secs, dark_window_timeout_secs, dark_window_default,
                status, activated_at, last_used_at, revoked_at, needs_review_since
             ) VALUES (?1, ?1, 1, 'email.create_draft', ?2, 5, ?3, 3, 3600,
                       ?4, NULL, NULL, 'active', ?5, NULL, NULL, NULL)",
            params![
                manifest.id,
                serde_json::to_string(&manifest).unwrap(),
                7 * 24 * 3600,
                7 * 24 * 3600,
                activated_at
            ],
        )
        .unwrap();
    });
    let boot_ms = Timestamp::now().as_millisecond();
    store.commit_boot_clock(boot_ms).unwrap();
    ops.record_terminal_erasure(&counterparty.to_string())
        .unwrap();
    drop(artifacts);
    drop(store);

    let reopened = Store::open(&db_path).unwrap();
    let reopened_artifacts =
        crate::artifact_store::ArtifactStore::open(data.join("artifacts"), *KEY).unwrap();
    validate_startup_and_reconcile_overlay_terminal_erasures(
        &reopened,
        &reopened_artifacts,
        &ops,
        &data,
        Timestamp::now().as_millisecond(),
    )
    .unwrap();

    let status: String = reopened.with_conn_for_test(|conn| {
        conn.query_row(
            "SELECT status FROM standing_rules WHERE rule_id = ?1",
            params![manifest.id],
            |row| row.get(0),
        )
        .unwrap()
    });
    assert_eq!(
        status, "revoked",
        "startup replay must sweep the erased counterparty out of scoped rules"
    );
    assert!(reopened.is_counterparty_erased(counterparty).unwrap());
}

#[test]
fn boot_clock_regression_prevents_erasure_reconciliation_and_audit() {
    let (_root, data) = temp_data_root();
    let db_path = data.join("kernel.db");
    let _ = fs::remove_file(&db_path);
    let store = Store::open(&db_path).unwrap();
    let artifacts =
        crate::artifact_store::ArtifactStore::open(data.join("artifacts"), *KEY).unwrap();
    let ops = acquire(&data, KEY).unwrap();
    ops.initialize_terminal_ledger()
        .expect("initialize terminal ledger");
    let high_water = 1_000_000_i64;
    store.commit_boot_clock(high_water).unwrap();

    let counterparty = Ulid::new();
    ops.record_terminal_erasure(&counterparty.to_string())
        .unwrap();

    let erased_before = store.erased_counterparty_ids().unwrap();
    let audit_before = store.all_audit_event_jsons().unwrap();
    let regressed = high_water - 60_001;
    let err = validate_startup_and_reconcile_overlay_terminal_erasures(
        &store, &artifacts, &ops, &data, regressed,
    )
    .expect_err("regressed boot clock must abort before reconciliation");

    assert!(
        err.to_string().contains("wall clock regressed at boot"),
        "unexpected error: {err}"
    );
    assert_eq!(
        store.erased_counterparty_ids().unwrap(),
        erased_before,
        "erasure rows must not be added on clock failure"
    );
    assert_eq!(
        store.all_audit_event_jsons().unwrap(),
        audit_before,
        "audit log must remain completely unmodified on clock failure"
    );
}

#[test]
fn broken_audit_chain_prevents_erasure_reconciliation_and_mutation() {
    let (_root, data) = temp_data_root();
    let db_path = data.join("kernel.db");
    let _ = fs::remove_file(&db_path);
    let store = Store::open(&db_path).unwrap();
    let artifacts =
        crate::artifact_store::ArtifactStore::open(data.join("artifacts"), *KEY).unwrap();
    let ops = acquire(&data, KEY).unwrap();
    ops.initialize_terminal_ledger()
        .expect("initialize terminal ledger");

    store
        .append_audit("system.boot", None, None, None, None, &[], &[])
        .unwrap();
    assert!(store.verify_audit_chain().unwrap());

    store.with_conn_for_test(|conn| {
        conn.execute(
            "UPDATE audit_log SET hash = 'sha256:0000000000000000000000000000000000000000000000000000000000000000' WHERE seq = 1",
            [],
        )
        .unwrap();
    });
    assert!(!store.verify_audit_chain().unwrap());

    let counterparty = Ulid::new();
    ops.record_terminal_erasure(&counterparty.to_string())
        .unwrap();

    let erased_before = store.erased_counterparty_ids().unwrap();
    let audit_before = store.all_audit_event_jsons().unwrap();
    let now_ms = Timestamp::now().as_millisecond();
    let err = validate_startup_and_reconcile_overlay_terminal_erasures(
        &store, &artifacts, &ops, &data, now_ms,
    )
    .expect_err("broken audit chain must abort before reconciliation");

    assert!(
        err.to_string().contains("audit_log hash chain is broken"),
        "unexpected error: {err}"
    );
    assert_eq!(
        store.erased_counterparty_ids().unwrap(),
        erased_before,
        "erasure rows must not be added on broken audit chain"
    );
    assert_eq!(
        store.all_audit_event_jsons().unwrap(),
        audit_before,
        "audit log must remain completely unmodified on broken audit chain"
    );
}

#[test]
fn finalization_audit_payload_contains_digest_and_no_bundle_name() {
    let (_root, data) = temp_data_root();
    let (ops, pending) = stage_export_pending(&data);
    let store = Store::open_in_memory().unwrap();
    let now = Timestamp::now();
    use sha2::{Digest as _, Sha256};
    let expected_digest: String = Sha256::digest(b"bundle-a")
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();

    let meta = ops.begin_finalization(&pending, now).unwrap();
    assert_eq!(meta.path_digest, expected_digest);
    append_overlay_finalization_audits(&store, &meta).unwrap();
    let event_jsons = store.all_audit_event_jsons().unwrap();
    assert_eq!(
        event_jsons.len(),
        2,
        "must append requested and terminal audit events"
    );

    for json_str in event_jsons {
        let audit_event: openspine_schemas::audit::AuditEvent =
            serde_json::from_str(&json_str).unwrap();
        let payload_str = audit_event
            .payload_json
            .expect("workflow step audit must have payload_json");
        let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
        let obj = payload.as_object().expect("payload must be a JSON object");

        assert!(
            !obj.contains_key("bundle_name"),
            "bundle_name key must be removed"
        );
        assert!(
            !payload_str.contains("bundle-a"),
            "plaintext bundle_name string 'bundle-a' must not appear in audit payload"
        );

        let path_digest = obj
            .get("path_digest")
            .and_then(|v| v.as_str())
            .expect("path_digest must be present in payload");
        assert_eq!(path_digest, expected_digest);
        assert_eq!(path_digest, meta.path_digest);

        let mut keys: Vec<String> = obj.keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                "action_id",
                "completed_at",
                "grant_id",
                "owner_principal_id",
                "path_digest",
                "request_id",
                "requested_at"
            ]
        );
    }
}
