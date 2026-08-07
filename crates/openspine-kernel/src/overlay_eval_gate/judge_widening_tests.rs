//! Direct tests for the scope-widening predicate (#133).
//!
//! The runtime path that raises `ScopeWidensActiveRule` cannot be constructed
//! today — see the doc comment on `widens` — so the predicate is tested here
//! rather than through a state that does not exist. Testing it directly is
//! what makes the guard a real rule instead of dead code nobody has read.

use std::collections::BTreeMap;

use openspine_schemas::action::ReviewedScopeDimension;
use openspine_schemas::event::AccountRole;
use openspine_schemas::reviewed_scope::ReviewedScopeValue;

use super::widens;

fn dimensions(
    pairs: &[(ReviewedScopeDimension, ReviewedScopeValue)],
) -> BTreeMap<ReviewedScopeDimension, ReviewedScopeValue> {
    pairs.iter().cloned().collect()
}

fn instance(name: &str) -> ReviewedScopeValue {
    ReviewedScopeValue::ConnectorInstance(name.to_string())
}

fn workflow(name: &str) -> ReviewedScopeValue {
    ReviewedScopeValue::Workflow(name.to_string())
}

#[test]
fn a_proposal_constraining_fewer_dimensions_widens_the_incumbent() {
    let proposed = dimensions(&[(ReviewedScopeDimension::ConnectorInstance, instance("a"))]);
    let incumbent = dimensions(&[
        (ReviewedScopeDimension::ConnectorInstance, instance("a")),
        (ReviewedScopeDimension::Workflow, workflow("w")),
    ]);
    assert!(
        widens(&proposed, &incumbent),
        "dropping a constraint the owner reviewed admits every context the \
         incumbent admits, plus more"
    );
}

#[test]
fn an_identical_scope_does_not_widen() {
    let scope = dimensions(&[
        (ReviewedScopeDimension::ConnectorInstance, instance("a")),
        (ReviewedScopeDimension::Workflow, workflow("w")),
    ]);
    assert!(
        !widens(&scope, &scope),
        "an identical scope is a supersession question, not a widening one"
    );
}

#[test]
fn a_proposal_constraining_more_dimensions_does_not_widen() {
    let proposed = dimensions(&[
        (ReviewedScopeDimension::ConnectorInstance, instance("a")),
        (ReviewedScopeDimension::Workflow, workflow("w")),
    ]);
    let incumbent = dimensions(&[(ReviewedScopeDimension::ConnectorInstance, instance("a"))]);
    assert!(
        !widens(&proposed, &incumbent),
        "narrowing is the safe direction and must not be refused"
    );
}

#[test]
fn a_disagreeing_shared_dimension_is_disjoint_not_widening() {
    let proposed = dimensions(&[(ReviewedScopeDimension::ConnectorInstance, instance("a"))]);
    let incumbent = dimensions(&[
        (ReviewedScopeDimension::ConnectorInstance, instance("b")),
        (ReviewedScopeDimension::Workflow, workflow("w")),
    ]);
    assert!(
        !widens(&proposed, &incumbent),
        "a different value on a shared dimension makes the scopes disjoint, \
         and disjoint rules coexist"
    );
}

#[test]
fn a_differently_typed_value_on_a_shared_dimension_is_not_widening() {
    let proposed = dimensions(&[(
        ReviewedScopeDimension::AccountRole,
        ReviewedScopeValue::AccountRole(AccountRole::OwnerMailbox),
    )]);
    let incumbent = dimensions(&[
        (
            ReviewedScopeDimension::AccountRole,
            ReviewedScopeValue::AccountRole(AccountRole::SharedWorkspaceMailbox),
        ),
        (ReviewedScopeDimension::Workflow, workflow("w")),
    ]);
    assert!(!widens(&proposed, &incumbent));
}
