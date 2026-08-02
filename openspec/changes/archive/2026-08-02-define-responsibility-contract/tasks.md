## 1. Contract-first tests

- [x] Add failing tests for incomplete action/implementation descriptors and unsafe communication defaults.
- [x] Add a synthetic non-Gmail scope derivation and generic mismatch test.
- [x] Add fail-closed tests for missing account scope and unresolved counterparties.
- [x] Add evidence-integrity tests for weak and duplicate repeated approvals.
- [x] Add channel-neutral owner-review and responsibility drift/reference-view tests.

## 2. Schemas and pure functions

- [x] Add action semantics, implementation, policy-bound, and reviewed-dimension schemas.
- [x] Add catalog lookup that fails closed unless both readiness axes validate.
- [x] Add sealed resolved context and versioned reviewed-scope derivation/comparison.
- [x] Add delegation evidence, owner review, responsibility, and compatibility-assessment schemas.
- [x] Register `email.create_draft` semantics without claiming a reusable implementation exists.

## 3. Architecture records

- [x] Add D-146 for the protocol-neutral responsibility contract and communication dark-window posture.
- [x] Amend the dependency-edged change sequence so downstream progressive-delegation slices depend on this contract.
- [x] Add and archive this OpenSpec change into canonical specs.

## 4. Verification

- [x] Run focused schema and catalog tests.
- [x] Run `cargo fmt --check` and clippy with warnings denied.
- [x] Run strict OpenSpec validation.
- [x] Remove the temporary source-export workflow before review.
- [x] Run every check in the complete `./scripts/check.sh` gate; split the workspace test run only where the local runner ceiling required it.
