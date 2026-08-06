use super::*;

#[test]
fn action_id_qualifier_is_part_of_identity() {
    let unqualified = ActionId::new("email.read_thread");
    let qualified = ActionId::new("email.read_thread:selected_no_attachments");
    assert_ne!(unqualified, qualified);
}

#[test]
fn action_id_serializes_as_bare_string() {
    let id = ActionId::new("telegram.reply:owner_channel");
    assert_eq!(
        serde_json::to_value(&id).unwrap(),
        serde_json::json!("telegram.reply:owner_channel")
    );
}

#[test]
fn gate_decision_round_trips_with_tag() {
    let decision = GateDecision::Deny {
        reason: DenialReason::ExplicitDeny,
    };
    let value = serde_json::to_value(&decision).unwrap();
    assert_eq!(value["outcome"], "deny");
    assert_eq!(value["reason"], "explicit_deny");
    let back: GateDecision = serde_json::from_value(value).unwrap();
    assert_eq!(decision, back);
}

#[test]
fn approval_required_never_serializes_as_allow() {
    let decision = GateDecision::ApprovalRequired {
        approval_type: "email.create_draft".to_string(),
    };
    let value = serde_json::to_value(&decision).unwrap();
    assert_eq!(value["outcome"], "approval_required");
    assert_ne!(value["outcome"], "allow");
}
