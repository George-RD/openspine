# Tasks: Implement gate action API

## 1. Types

- [ ] Define action request type.
- [ ] Define gate decision type.
- [ ] Define denial reason enum or equivalent.
- [ ] Define approval-required decision shape.
- [ ] Define audit metadata shape.

## 2. Gate implementation

- [ ] Implement denied-action check.
- [ ] Implement approval-required check.
- [ ] Implement allowed-action check.
- [ ] Implement unspecified deny.
- [ ] Return structured decision.

## 3. Audit

- [ ] Emit or return audit metadata for every decision.
- [ ] Ensure private payloads are refs/hashes only.
- [ ] Add denial audit examples.

## 4. Tests

- [ ] Test allowed action returns allow.
- [ ] Test denied action returns deny.
- [ ] Test approval-required action returns approval-required.
- [ ] Test allowed plus denied returns deny.
- [ ] Test allowed plus approval-required returns approval-required.
- [ ] Test unspecified action returns deny.
- [ ] Test approval-required action does not execute.

## 5. Validation

- [ ] Run unit tests.
- [ ] Run `openspec validate --changes implement-gate-action-api --strict`.
