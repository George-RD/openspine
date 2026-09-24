use super::*;
use openspine_schemas::artifact::Lifecycle;
use openspine_schemas::route::{Route, RouteEffect};

fn route(id: &str) -> Route {
    Route {
        id: id.into(),
        schema_version: 1,
        version: 1,
        lifecycle_state: Lifecycle::Active,
        priority: Some(1),
        effect: RouteEffect::Allow,
        when: Default::default(),
        agent: None,
        workflow: None,
        capability_pack: None,
        persona: None,
    }
}

#[test]
fn typed_route_change_exposes_complete_before_and_after_bindings() {
    let mut before = ArtifactRegistry::default();
    before.routes.push(route("owner-route"));
    let mut after = before.clone();
    after.routes[0].effect = RouteEffect::Deny;
    after.routes[0].priority = Some(9);
    after.routes[0].when.channel_account = Some("owner-account".into());
    let result = compare(&before, &after);
    assert_ne!(result.before_runtime_digest, result.after_runtime_digest);
    assert_eq!(result.changes.len(), 1);
    let change = &result.changes[0];
    assert_eq!(change.kind, "route");
    assert_eq!(change.id, "owner-route");
    assert_eq!(change.before.as_ref().unwrap()["fields"]["effect"], "allow");
    assert_eq!(change.after.as_ref().unwrap()["fields"]["effect"], "deny");
    assert_eq!(change.after.as_ref().unwrap()["fields"]["priority"], 9);
    assert_eq!(
        change.after.as_ref().unwrap()["fields"]["when"]["channel_account"],
        "owner-account"
    );
}
