use super::*;
use crate::artifact::Lifecycle;
use jiff::Timestamp;

fn sample_root() -> TaskGrant {
    let issued_at = Timestamp::now();
    let id = Ulid::new();
    let token = Ulid::new();
    let mut grant = TaskGrant {
        id,
        schema_version: 1,
        lifecycle_state: Lifecycle::Active,
        user: "owner".to_string(),
        purpose: "test".to_string(),
        issued_by: "kernel".to_string(),
        issued_at,
        expires_at: issued_at + std::time::Duration::from_secs(120),
        event_id: Ulid::new(),
        route_id: "r".to_string(),
        agent_id: "a".to_string(),
        workflow_id: "w".to_string(),
        capability_pack_id: "p".to_string(),
        authority_sources: vec![],
        selection_tokens: vec![token],
        allowed_actions: vec![
            ActionId::new("openspine.status.read"),
            ActionId::new("lyra.ui.preview"),
        ],
        approval_required_actions: vec![],
        denied_actions: vec![],
        allowed_egress_classes: vec![],
        output_channels: vec!["telegram.owner.reply".to_string()],
        limits: GrantLimits {
            max_model_calls: 8,
            max_artifacts: 20,
            max_runtime_seconds: 120,
        },
        task_token: "a".repeat(64),
        root_grant_id: id,
        parent_grant_id: None,
        mode: GrantMode::Live,
        chain: vec![],
        caveat_mac: String::new(),
        thread_id: None,
    };
    seal_root(&mut grant, TEST_GRANT_HMAC_KEY);
    grant
}

/// Reconstruct the root payload shape used before AD-060 added the egress
/// field. This intentionally mirrors the pre-change implementation so the
/// compatibility regression below proves an old persisted MAC still verifies.
fn pre_egress_root_bytes(root: &RootAuthority) -> Vec<u8> {
    let mut allowed: Vec<String> = root
        .allowed_actions
        .iter()
        .map(|a| a.as_str().to_string())
        .collect();
    allowed.sort();
    let mut approval: Vec<String> = root
        .approval_required_actions
        .iter()
        .map(|a| a.as_str().to_string())
        .collect();
    approval.sort();
    let mut denied: Vec<String> = root
        .denied_actions
        .iter()
        .map(|a| a.as_str().to_string())
        .collect();
    denied.sort();
    let mut channels = root.output_channels.clone();
    channels.sort();
    let payload = serde_json::json!({
        "root_grant_id": root.root_grant_id.to_string(),
        "expires_at": root.expires_at.to_string(),
        "allowed_actions": allowed,
        "approval_required_actions": approval,
        "denied_actions": denied,
        "output_channels": channels,
        "limits": {
            "max_model_calls": root.limits.max_model_calls,
            "max_artifacts": root.limits.max_artifacts,
            "max_runtime_seconds": root.limits.max_runtime_seconds,
        },
        "user": root.user,
        "purpose": root.purpose,
        "event_id": root.event_id.to_string(),
        "route_id": root.route_id,
        "agent_id": root.agent_id,
        "workflow_id": root.workflow_id,
        "capability_pack_id": root.capability_pack_id,
    });
    canonical_json(&payload).into_bytes()
}

fn pre_egress_mac_hex(root: &RootAuthority, chain: &[ChainStep]) -> String {
    let mut tip = hmac_sha256(TEST_GRANT_HMAC_KEY, &pre_egress_root_bytes(root));
    for step in chain {
        for caveat in &step.added_caveats {
            tip = hmac_sha256(&tip, &caveat_bytes(caveat));
        }
        tip = hmac_sha256(&tip, &step_bind_bytes(step));
    }
    hex_encode(&tip)
}

#[test]
fn root_seals_and_verifies() {
    let grant = sample_root();
    assert!(verify_mac(TEST_GRANT_HMAC_KEY, &grant));
}

#[test]
fn pre_egress_grant_mac_still_verifies_after_upgrade() {
    // This is the exact shape of a grant persisted by the pre-AD-060
    // binary: no egress key in the root payload and an empty class list
    // supplied only by the post-upgrade in-memory schema default.
    let mut grant = sample_root();
    let root = RootAuthority::from_grant(&grant);
    grant.caveat_mac = pre_egress_mac_hex(&root, &grant.chain);
    assert!(
        verify_mac(TEST_GRANT_HMAC_KEY, &grant),
        "legacy grant MAC must remain valid across an AD-060 upgrade"
    );
}

#[test]
fn egress_class_tamper_invalidates_root_mac() {
    let mut grant = sample_root();
    grant.allowed_egress_classes = vec![crate::egress::EgressClass::Search];
    seal_root(&mut grant, TEST_GRANT_HMAC_KEY);
    assert!(verify_mac(TEST_GRANT_HMAC_KEY, &grant));

    grant
        .allowed_egress_classes
        .push(crate::egress::EgressClass::WebFormPost);
    assert!(
        !verify_mac(TEST_GRANT_HMAC_KEY, &grant),
        "adding an egress class must invalidate the MAC"
    );
}

#[test]
fn legacy_grant_without_thread_id_still_verifies() {
    let grant = sample_root();
    let canonical = String::from_utf8(RootAuthority::from_grant(&grant).canonical_bytes())
        .expect("canonical root is UTF-8 JSON");
    assert!(
        !canonical.contains("thread_id"),
        "legacy None root form must omit the optional binding"
    );
    let mut legacy = serde_json::to_value(&grant).unwrap();
    legacy
        .as_object_mut()
        .expect("grant serializes as object")
        .remove("thread_id");
    let decoded: TaskGrant = serde_json::from_value(legacy).unwrap();
    assert!(decoded.thread_id.is_none());
    assert!(verify_mac(TEST_GRANT_HMAC_KEY, &decoded));
}

#[test]
fn adding_thread_binding_without_resealing_invalidates_mac() {
    let grant = sample_root();
    let mut tampered = grant.clone();
    tampered.thread_id = Some("topic-42".to_string());
    assert!(!verify_mac(TEST_GRANT_HMAC_KEY, &tampered));
}

#[test]
fn child_derives_from_parent_tip_without_root_key() {
    let parent = sample_root();
    let child_id = Ulid::new();
    let child_step = ChainStep {
        grant_id: child_id,
        parent_grant_id: Some(parent.id),
        mode: GrantMode::Live,
        selection_tokens: vec![],
        added_caveats: vec![Caveat::ActionAllowlist {
            actions: vec![ActionId::new("openspine.status.read")],
        }],
    };
    let child_mac = seal_child_from_parent_tip(&parent.caveat_mac, &child_step).unwrap();
    let mut child = parent.clone();
    child.id = child_id;
    child.parent_grant_id = Some(parent.id);
    child.selection_tokens = vec![];
    child.task_token = "b".repeat(64);
    child.chain.push(child_step);
    child.caveat_mac = child_mac;
    assert!(verify_mac(TEST_GRANT_HMAC_KEY, &child));
    assert!(effectively_allows(
        &child,
        &ActionId::new("openspine.status.read")
    ));
    assert!(!effectively_allows(
        &child,
        &ActionId::new("lyra.ui.preview")
    ));
}

#[test]
fn selection_token_tamper_invalidates_mac() {
    let grant = sample_root();
    let mut tampered = grant.clone();
    tampered.selection_tokens.push(Ulid::new());
    // Structural mismatch with chain step OR MAC fail.
    assert!(!verify_mac(TEST_GRANT_HMAC_KEY, &tampered));
    // Even if chain step is also widened without resealing:
    tampered.chain[0].selection_tokens = tampered.selection_tokens.clone();
    assert!(!verify_mac(TEST_GRANT_HMAC_KEY, &tampered));
}

#[test]
fn id_parent_and_action_list_tamper_fail() {
    let parent = sample_root();
    let child_id = Ulid::new();
    let child_step = ChainStep {
        grant_id: child_id,
        parent_grant_id: Some(parent.id),
        mode: GrantMode::Live,
        selection_tokens: vec![],
        added_caveats: vec![Caveat::ActionAllowlist {
            actions: vec![ActionId::new("openspine.status.read")],
        }],
    };
    let mac = seal_child_from_parent_tip(&parent.caveat_mac, &child_step).unwrap();
    let mut child = parent.clone();
    child.id = child_id;
    child.parent_grant_id = Some(parent.id);
    child.selection_tokens = vec![];
    child.chain.push(child_step);
    child.caveat_mac = mac;
    assert!(verify_mac(TEST_GRANT_HMAC_KEY, &child));
    let mut id_tamper = child.clone();
    id_tamper.id = Ulid::new();
    assert!(!verify_mac(TEST_GRANT_HMAC_KEY, &id_tamper));
    let mut parent_tamper = child.clone();
    parent_tamper.parent_grant_id = Some(Ulid::new());
    assert!(!verify_mac(TEST_GRANT_HMAC_KEY, &parent_tamper));
    let mut widened = child.clone();
    widened.allowed_actions.push(ActionId::new("email.send"));
    assert!(!verify_mac(TEST_GRANT_HMAC_KEY, &widened));
}

#[test]
fn caveat_reorder_or_remove_invalidates_mac() {
    let parent = sample_root();
    let child_id = Ulid::new();
    let caveats = vec![
        Caveat::ActionAllowlist {
            actions: vec![ActionId::new("openspine.status.read")],
        },
        Caveat::BoundParameter {
            name: "thread".into(),
            value: "t1".into(),
        },
        Caveat::ExpiresBefore {
            at: parent.expires_at,
        },
    ];
    let child_step = ChainStep {
        grant_id: child_id,
        parent_grant_id: Some(parent.id),
        mode: GrantMode::Live,
        selection_tokens: vec![],
        added_caveats: caveats.clone(),
    };
    let mac = seal_child_from_parent_tip(&parent.caveat_mac, &child_step).unwrap();
    let mut child = parent.clone();
    child.id = child_id;
    child.parent_grant_id = Some(parent.id);
    child.selection_tokens = vec![];
    child.chain.push(child_step);
    child.caveat_mac = mac;
    assert!(verify_mac(TEST_GRANT_HMAC_KEY, &child));

    // Reorder caveats without resealing.
    let mut reordered = child.clone();
    reordered.chain.last_mut().unwrap().added_caveats =
        vec![caveats[1].clone(), caveats[0].clone(), caveats[2].clone()];
    assert!(!verify_mac(TEST_GRANT_HMAC_KEY, &reordered));

    // Remove a caveat without resealing.
    let mut removed = child.clone();
    removed.chain.last_mut().unwrap().added_caveats.pop();
    assert!(!verify_mac(TEST_GRANT_HMAC_KEY, &removed));
}

#[test]
fn identity_field_tamper_invalidates_mac() {
    let grant = sample_root();
    assert!(verify_mac(TEST_GRANT_HMAC_KEY, &grant));
    for mutate in [
        |g: &mut TaskGrant| g.user = "attacker".into(),
        |g: &mut TaskGrant| g.purpose = "widened".into(),
        |g: &mut TaskGrant| g.agent_id = "evil_agent".into(),
        |g: &mut TaskGrant| g.route_id = "evil_route".into(),
        |g: &mut TaskGrant| g.event_id = Ulid::new(),
    ] {
        let mut tampered = grant.clone();
        mutate(&mut tampered);
        assert!(
            !verify_mac(TEST_GRANT_HMAC_KEY, &tampered),
            "identity/root field tamper must fail MAC"
        );
    }
}
