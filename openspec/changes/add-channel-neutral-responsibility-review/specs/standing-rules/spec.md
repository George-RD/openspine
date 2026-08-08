# standing-rules Specification Delta

## MODIFIED Requirements

### Requirement: Standing rules are reviewed composition inputs

The kernel MUST represent a standing rule as a versioned, revocable, expiring artifact. A rule MUST enter live consultation only after the existing proposal, evaluation, owner-approval, and activation ceremony. A standing rule MUST NOT replace or widen the authenticated task grant.

A rule MAY be paused by the owner. A paused rule MUST be absent from live consultation, so the action it covers requires ordinary owner approval while paused. Pause, resume, and revoke MUST be owner-controlled transitions, each writing a distinct audit event and each replay-safe (a duplicate or concurrent transition is a safe no-op). A new version activating over a paused rule MUST supersede it so a stale paused version cannot silently reappear on a later resume.

#### Scenario: Reviewed rule becomes a live input

- **WHEN** an approved standing-rule artifact is activated
- **THEN** a matching action that otherwise requires approval MAY be admitted within the rule budget
- **AND** the task grant remains the live authority object
- **AND** `standing_rule_activation_ceremony_reaches_live_consultation` MUST pass

#### Scenario: Revoked or expired rule is absent

- **WHEN** a rule is revoked or lapses after its expiry interval
- **THEN** live consultation MUST return no matching rule
- **AND** normal owner approval remains required
- **AND** `artifact_revoke_dispatch_removes_rule_from_live_consultation` and `standing_rule_lapses_after_expiry_unused` MUST pass

#### Scenario: Paused rule is absent from live consultation

- **WHEN** an active rule is paused
- **THEN** the rule MUST leave live consultation immediately
- **AND** the covered action MUST require ordinary owner approval while paused
- **AND** no scoped admission, no dark-window timer, and no pending exception MAY act on the paused rule

#### Scenario: Duplicate pause or revoke is replay-safe

- **WHEN** a pause or revoke intent is delivered more than once
- **THEN** exactly one durable audit disposition MUST be written
- **AND** the rule MUST NOT double-transition

#### Scenario: A new version supersedes a paused rule

- **WHEN** a newer version of a rule activates while an older version is paused
- **THEN** the paused older version MUST transition to revoked
- **AND** a later resume MUST fail the version-staleness re-check

## ADDED Requirements

### Requirement: Resume MUST revalidate compatibility before reactivating

A paused rule MAY return to `active` only through a compatibility-revalidated resume. Resume MUST re-verify the reviewed bytes and binding digest, re-check that the rule is still the exact version that was paused, and revalidate policy, descriptor, executor readiness, connector/account health, and reviewed scope before returning the rule to `active`. The reviewed-scope revalidation at resume MUST re-verify that the persisted binding is internally consistent (its stored values still agree with its stored digest) and that the rule's bound compatibility epoch still equals the catalog's current declaration axes; a corrupt persisted scope binding MUST surface as the invalid-scope outcome rather than matching on either half. Resume deliberately does NOT compare context dimensions through `ReviewedActionScope::compare`: resume is owner-initiated and has no inbound action request, so there is no freshly resolved context to compare against. Context-dimension comparison stays where a resolved context exists — scoped admission at consultation time — and remains the single canonical use of `ReviewedActionScope::compare`. Resume MUST NOT add connector-specific branches into generic resume code; connector health MUST be assessed through the connector registry's breaker state and account/credential validity per connector.

A failed resume MUST leave the rule in the `paused` state (it does not return to `active` and does not move to `needs_review`), MUST require a new reviewed version, and MUST write a distinct audit event naming the rejection reason. A concurrent resume tap MUST be a safe no-op.

#### Scenario: Resume reactivates a still-current reviewed version

- **WHEN** a paused rule is unchanged, unexpired, undrifted, and its connector and account remain valid
- **THEN** resume MUST return the rule to `active` after re-validating bytes, digest, version, policy, descriptor, executor readiness, connector/account, and reviewed scope
- **AND** the covered action MAY again be admitted within the rule budget

#### Scenario: Resume refuses an expired rule

- **WHEN** a paused rule has lapsed after its expiry interval
- **THEN** resume MUST refuse to reactivate the rule
- **AND** the rule MUST remain `paused`
- **AND** a new reviewed version MUST be required
- **AND** an audit event with reason `resume_refused_expired` MUST be written

#### Scenario: Resume refuses a superseded rule

- **WHEN** a newer version of the rule activated while the older version was paused
- **THEN** resume MUST refuse to reactivate the older version
- **AND** the rule MUST remain `paused`
- **AND** a new reviewed version MUST be required
- **AND** an audit event with reason `resume_refused_superseded` MUST be written

#### Scenario: Resume refuses a drifted reviewed scope

- **WHEN** the rule's bound compatibility epoch no longer equals the catalog's current declaration axes
- **THEN** resume MUST refuse to reactivate the rule
- **AND** the rule MUST remain `paused`
- **AND** a new reviewed version MUST be required
- **AND** an audit event with reason `resume_refused_scope_drift` MUST be written

#### Scenario: Resume refuses an unavailable executor or connector

- **WHEN** the action's executor is not registered or the connector/account is unhealthy
- **THEN** resume MUST refuse to reactivate the rule
- **AND** the rule MUST remain `paused`
- **AND** a new reviewed version MUST be required
- **AND** an audit event with reason `resume_refused_unavailable` MUST be written

#### Scenario: Resume refuses a corrupt scope binding

- **WHEN** the persisted reviewed-scope binding is internally inconsistent
- **THEN** resume MUST refuse to reactivate the rule
- **AND** the rule MUST remain `paused`
- **AND** an audit event with reason `resume_refused_invalid_scope` MUST be written

#### Scenario: Concurrent resume taps are replay-safe

- **WHEN** two resume intents race for the same paused rule
- **THEN** exactly one reactivation MUST win
- **AND** the other MUST be a safe no-op
