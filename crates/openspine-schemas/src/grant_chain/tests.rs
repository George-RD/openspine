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
    };
    seal_root(&mut grant, TEST_GRANT_HMAC_KEY);
    grant
}

#[test]
fn root_seals_and_verifies() {
    let grant = sample_root();
    assert!(verify_mac(TEST_GRANT_HMAC_KEY, &grant));
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
