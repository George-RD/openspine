use super::*;
use openspine_schemas::action::ActionEgressDeclaration;

#[test]
fn bound_parameter_conflict_is_caveat_widening() {
    // Valid MAC over a chain that contains conflicting AD-036 bindings, so
    // the failure comes from bindings_valid — not a short-circuit MAC miss.
    let key = b"openspine-test-grant-hmac-key-v1";
    let mut grant = grant_with(&["openspine.status.read"], &[], &[]);
    grant.chain = vec![openspine_schemas::grant::GrantChainStep {
        grant_id: grant.id,
        parent_grant_id: None,
        mode: GrantMode::Live,
        selection_tokens: grant.selection_tokens.clone(),
        added_caveats: vec![
            openspine_schemas::grant::GrantCaveat::BoundParameter {
                name: "recipient".into(),
                value: "a@example.com".into(),
            },
            openspine_schemas::grant::GrantCaveat::BoundParameter {
                name: "recipient".into(),
                value: "b@example.com".into(),
            },
        ],
    }];
    grant.root_grant_id = grant.id;
    let root = openspine_schemas::grant_chain::RootAuthority::from_grant(&grant);
    grant.caveat_mac = openspine_schemas::grant_chain::compute_mac_hex(key, &root, &grant.chain);
    assert!(
        grant.verify_mac(key),
        "precondition: MAC must be valid so bindings_valid is the deny path"
    );
    assert!(!openspine_schemas::grant_chain::bindings_valid(&grant));
    let outcome = gate(
        &grant,
        &request_for("openspine.status.read"),
        ActionOrigin::Shell,
        &MockContext::default(),
        &test_catalog(),
        &NoEgress,
        Timestamp::now(),
    );
    assert_eq!(
        outcome.decision,
        GateDecision::Deny {
            reason: DenialReason::CaveatWidening
        }
    );
}

struct KernelTrustedRated;

impl EgressClassifier for KernelTrustedRated {
    fn classify(&self, action: &ActionId) -> Option<EgressClass> {
        (action == &ActionId::new("trusted.web.form_submit")).then_some(EgressClass::WebFormPost)
    }
}

#[test]
fn kernel_origin_cannot_bypass_rated_egress_class() {
    let grant = grant_with(&["trusted.web.form_submit"], &[], &[]);
    let action = ActionId::new("trusted.web.form_submit");
    let catalog = ActionCatalog::new([action.clone()])
        .with_kernel_origin([action.clone()])
        .with_egress_declarations([(
            action.clone(),
            ActionEgressDeclaration {
                output_channels: None,
                egress_class: Some(EgressClass::WebFormPost),
            },
        )]);
    let outcome = gate(
        &grant,
        &request_for("trusted.web.form_submit"),
        ActionOrigin::Kernel,
        &MockContext::default(),
        &catalog,
        &KernelTrustedRated,
        Timestamp::now(),
    );
    assert_eq!(
        outcome.decision,
        GateDecision::Deny {
            reason: DenialReason::EgressClassNotGranted
        }
    );
}

#[test]
fn explicit_shell_deny_precedes_uncovered_egress_class() {
    let grant = grant_with(
        &["trusted.web.form_submit"],
        &[],
        &["trusted.web.form_submit"],
    );
    let action = ActionId::new("trusted.web.form_submit");
    let catalog = ActionCatalog::new([action.clone()]).with_egress_declarations([(
        action.clone(),
        ActionEgressDeclaration {
            output_channels: None,
            egress_class: Some(EgressClass::WebFormPost),
        },
    )]);
    let outcome = gate(
        &grant,
        &request_for("trusted.web.form_submit"),
        ActionOrigin::Shell,
        &MockContext::default(),
        &catalog,
        &KernelTrustedRated,
        Timestamp::now(),
    );
    assert_eq!(
        outcome.decision,
        GateDecision::Deny {
            reason: DenialReason::ExplicitDeny
        }
    );
}

#[test]
fn bound_parameter_allows_when_request_matches() {
    // Grant carries a BoundParameter caveat; request carries the matching
    // param value → gate allows.
    let key = b"openspine-test-grant-hmac-key-v1";
    let mut grant = grant_with(&["openspine.status.read"], &[], &[]);
    grant.chain = vec![openspine_schemas::grant::GrantChainStep {
        grant_id: grant.id,
        parent_grant_id: None,
        mode: GrantMode::Live,
        selection_tokens: grant.selection_tokens.clone(),
        added_caveats: vec![openspine_schemas::grant::GrantCaveat::BoundParameter {
            name: "recipient".into(),
            value: "alice@example.com".into(),
        }],
    }];
    grant.root_grant_id = grant.id;
    let root = openspine_schemas::grant_chain::RootAuthority::from_grant(&grant);
    grant.caveat_mac = openspine_schemas::grant_chain::compute_mac_hex(key, &root, &grant.chain);
    assert!(grant.verify_mac(key), "precondition: MAC must be valid");
    let mut req = request_for("openspine.status.read");
    req.params
        .insert("recipient".into(), "alice@example.com".into());
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &MockContext::default(),
        &test_catalog(),
        &NoEgress,
        Timestamp::now(),
    );
    assert_eq!(
        outcome.decision,
        GateDecision::Allow,
        "matching bound parameter must allow"
    );
}

#[test]
fn bound_parameter_denies_when_request_missing() {
    // Grant carries a BoundParameter caveat; request has no params at all
    // → gate denies with CaveatWidening.
    let key = b"openspine-test-grant-hmac-key-v1";
    let mut grant = grant_with(&["openspine.status.read"], &[], &[]);
    grant.chain = vec![openspine_schemas::grant::GrantChainStep {
        grant_id: grant.id,
        parent_grant_id: None,
        mode: GrantMode::Live,
        selection_tokens: grant.selection_tokens.clone(),
        added_caveats: vec![openspine_schemas::grant::GrantCaveat::BoundParameter {
            name: "recipient".into(),
            value: "alice@example.com".into(),
        }],
    }];
    grant.root_grant_id = grant.id;
    let root = openspine_schemas::grant_chain::RootAuthority::from_grant(&grant);
    grant.caveat_mac = openspine_schemas::grant_chain::compute_mac_hex(key, &root, &grant.chain);
    assert!(grant.verify_mac(key), "precondition: MAC must be valid");
    let outcome = gate(
        &grant,
        &request_for("openspine.status.read"),
        ActionOrigin::Shell,
        &MockContext::default(),
        &test_catalog(),
        &NoEgress,
        Timestamp::now(),
    );
    assert_eq!(
        outcome.decision,
        GateDecision::Deny {
            reason: DenialReason::CaveatWidening
        },
        "missing bound parameter must deny"
    );
}

#[test]
fn bound_parameter_denies_when_request_mismatched() {
    // Grant carries a BoundParameter caveat; request has a different value
    // → gate denies with CaveatWidening.
    let key = b"openspine-test-grant-hmac-key-v1";
    let mut grant = grant_with(&["openspine.status.read"], &[], &[]);
    grant.chain = vec![openspine_schemas::grant::GrantChainStep {
        grant_id: grant.id,
        parent_grant_id: None,
        mode: GrantMode::Live,
        selection_tokens: grant.selection_tokens.clone(),
        added_caveats: vec![openspine_schemas::grant::GrantCaveat::BoundParameter {
            name: "recipient".into(),
            value: "alice@example.com".into(),
        }],
    }];
    grant.root_grant_id = grant.id;
    let root = openspine_schemas::grant_chain::RootAuthority::from_grant(&grant);
    grant.caveat_mac = openspine_schemas::grant_chain::compute_mac_hex(key, &root, &grant.chain);
    assert!(grant.verify_mac(key), "precondition: MAC must be valid");
    let mut req = request_for("openspine.status.read");
    req.params
        .insert("recipient".into(), "bob@example.com".into());
    let outcome = gate(
        &grant,
        &req,
        ActionOrigin::Shell,
        &MockContext::default(),
        &test_catalog(),
        &NoEgress,
        Timestamp::now(),
    );
    assert_eq!(
        outcome.decision,
        GateDecision::Deny {
            reason: DenialReason::CaveatWidening
        },
        "mismatched bound parameter must deny"
    );
}
