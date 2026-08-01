use openspine_schemas::action::ActionId;
use serde_json::json;

use super::actions::DispatchError;
use super::artifact_propose::dispatch_artifact_propose;
use super::dispatch_tests::OWNER_CHAT_ID;
use crate::pipeline::handle_owner_update;
use crate::test_support::fixtures::{owner_update, seed_owner_history, test_state};

#[tokio::test]
async fn standing_rule_for_action_denied_by_active_policy_is_rejected_before_persistence() {
    let state = test_state();
    let grant = handle_owner_update(&state, &owner_update("hello lyra"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    seed_owner_history(&state, &grant);

    let yaml = concat!(
        "id: email_send_responsibility\n",
        "schema_version: 1\n",
        "version: 1\n",
        "lifecycle_state: proposed\n",
        "action_id: email.send\n",
        "description: Send routine email replies\n",
        "quota: {max: 5, window_secs: 604800}\n",
        "rate: {max: 1, window_secs: 3600}\n",
        "expires_after_secs: 7776000\n",
    );
    let payload = json!({"kind": "standing_rule", "yaml": yaml});

    let err = dispatch_artifact_propose(
        &state,
        &grant,
        &ActionId::new("artifact.propose"),
        OWNER_CHAT_ID,
        Some(&payload),
    )
    .await
    .expect_err("an active policy deny must make the responsibility impossible to approve");

    match err {
        DispatchError::BadRequest(message) => assert!(
            message.contains("denied by active policy"),
            "unexpected message: {message}"
        ),
        DispatchError::Resource(_)
        | DispatchError::Connector(_)
        | DispatchError::ConnectorUnavailable(_)
        | DispatchError::DeliveryUnknown(_) => {
            panic!("a policy-denied standing rule must fail before persistence or delivery")
        }
    }
    assert!(!state
        .store
        .proposed_artifact_exists("standing_rule", "email_send_responsibility", 1)
        .unwrap());
}
