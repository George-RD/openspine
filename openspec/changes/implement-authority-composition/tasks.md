# Tasks: Implement authority composition

## 1. Composer interface

- [ ] Define authority composition input type.
- [ ] Define authority composition result type.
- [ ] Define task grant output type.
- [ ] Include allowed, denied, and approval-required action sets.

## 2. Merge logic

- [ ] Implement deny-by-default start state.
- [ ] Gather candidate allows from route, workflow, agent manifest, and capability pack.
- [ ] Intersect with global and user/session policy.
- [ ] Apply lane, connector, account-role, data-class, channel, and task constraints.
- [ ] Apply explicit deny precedence.
- [ ] Apply approval-required precedence.
- [ ] Materialize task grant.

## 3. Tests

- [ ] Test that no action is allowed without candidate allow.
- [ ] Test explicit deny overrides allow.
- [ ] Test approval-required overrides plain allow.
- [ ] Test identity alone grants no authority.
- [ ] Test connector/account role alone grants no authority.
- [ ] Test main assistant does not inherit specialist workflow authority.
- [ ] Test selected-thread email grant excludes inbox-wide read.
- [ ] Test authority widening requires approval.

## 4. Documentation

- [ ] Document authority composition rule.
- [ ] Document task grant as the final authority object.
- [ ] Link to relevant decision-log entries.

## 5. Validation

- [ ] Run unit tests.
- [ ] Run `openspec validate --changes implement-authority-composition --strict`.
