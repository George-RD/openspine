//! End-to-end proof that the capability-derived tool catalog served on
//! `GET /v1/task` (spec #209, IT3) is advisory: `gate()` remains the sole
//! enforcement point. Split from `tests.rs` to keep that file under the repo's
//! 500-line-per-file gate; reuses its `start_server` / `post_action` harness.

use serde_json::Value;

use super::tests::{post_action, start_server};
use crate::pipeline::handle_owner_update;
use crate::test_support::fixtures::{owner_update, test_state};

/// IT3 (spec #209 Q5/D-055): structural absence is attenuation; gate() is the
/// sole enforcement. An action absent from a grant's `/v1/task` catalog is
/// still refused by `gate()` when POSTed to `/v1/actions` — the catalog is
/// advisory, never an enforcement mechanism.
#[tokio::test]
async fn action_absent_from_catalog_is_still_denied_by_gate() {
    let state = test_state();
    let grant = handle_owner_update(&state, &owner_update("check inbox"))
        .await
        .unwrap()
        .expect("owner update must compose a grant");
    let store = state.store.clone();
    let (addr, handle) = start_server(state).await;

    // The catalog served to this grant does not name `email.read_inbox` (a
    // denied action) — asserted positively over the wire.
    let task_view: Value = reqwest::Client::new()
        .get(format!("http://{addr}/v1/task"))
        .bearer_auth(&grant.task_token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let catalog_ids: Vec<&str> = task_view["catalog"]["tools"]
        .as_array()
        .expect("catalog.tools must be an array")
        .iter()
        .map(|tool| tool["action_id"].as_str().unwrap())
        .collect();
    assert!(
        !catalog_ids.contains(&"email.read_inbox"),
        "a denied action must be structurally absent from the catalog: {catalog_ids:?}"
    );

    // structural absence is attenuation; gate() is the sole enforcement
    // (spec Q5/D-055): POSTing the absent action is still denied by gate().
    let resp = post_action(addr, &grant.task_token, "email.read_inbox", None).await;
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["decision"]["outcome"], "deny");
    assert!(body.get("result").is_none());

    // The refusal is a real gate() decision, recorded as an `action.gated`
    // audit row for the absent action.
    let gated = store
        .all_audit_event_jsons()
        .unwrap()
        .into_iter()
        .map(|json| serde_json::from_str::<Value>(&json).unwrap())
        .find(|event| event["kind"] == "action.gated" && event["action"] == "email.read_inbox")
        .expect("a gate() decision audit row must exist for the absent action");
    assert_eq!(gated["decision"]["outcome"], "deny");

    handle.abort();
}
