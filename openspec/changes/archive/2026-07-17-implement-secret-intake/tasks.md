# Tasks: Implement secret intake

## 1. OpenSpec and authority

- [x] Read D-014, D-005, D-010, D-025, D-004, D-006, D-007, D-008, D-011, and D-012.
- [x] Document the metadata-only gate transition and direct next-message capture.
- [x] Add any required decision-log clarification as proposed text in IMPLEMENTATION-NOTES.md; canon files are out of scope for this worktree.

## 2. Encrypted vault

- [x] Add `SecretStore` under `crates/openspine-kernel/src/secret_store.rs`, mirroring `ArtifactStore` AES-256-GCM and nonce handling.
- [x] Implement validated slot names, atomic overwrite/rotation, decrypt failure handling, and idempotent first-run `seed_if_absent`.
- [x] Add tests for introduction, rotation, corrupted/truncated ciphertext, and invalid slot names.

## 3. Gate-mediated intake mode

- [x] Add `secret.intake` and `secret.rotate` to the ordinary action catalog; do not add them to the kernel-origin trusted set.
- [x] Parse exact `/secret intake <slot>` and `/secret rotate <slot>` commands.
- [x] Construct a short-lived HMAC-sealed owner grant scoped to exactly one action and pass its metadata-only request through `gate()` with `ActionOrigin::Shell`.
- [x] Persist pending slot/mode/chat/grant/request/expiry metadata only after `GateDecision::Allow`.
- [x] Capture the next verified message before normal pipeline routing; validate chat binding and expiry before reading it as a credential.
- [x] Discard expired/mismatched pending messages from normal routing and return metadata-only retry feedback.

## 4. Connector wiring

- [x] Seed Telegram and Gmail connector bootstrap env values only when their vault slots are absent.
- [x] Resolve Gmail OAuth values at token-refresh call time and invalidate cached access tokens after credential rotation.
- [x] Resolve Telegram bot token on every poll/send/callback call and rebuild `Bot` only when the token changes.
- [x] Keep provider API-key migration and artifact-root-key rotation out of scope.

## 5. Acceptance tests

- [x] Prove the same connector instance uses value A, then rotated value B, without process restart (Gmail wiremock regression test).
- [x] Prove secret plaintext never appears in action/audit metadata or owner response (pre-pipeline capture test; the HTTP contract has no secret-bearing field).
- [x] Prove malformed/stale pending state is consumed and cannot replay into normal routing.

## 6. Verification

- [x] `cargo fmt && cargo fmt --check`.
- [x] `cargo clippy --workspace --all-targets -- -D warnings`.
- [x] `cargo test --workspace`.
- [x] `bash scripts/check-file-sizes.sh`.
- [x] `/Users/george/repos/openspine/node_modules/.bin/openspec validate implement-secret-intake --strict`.
- [x] Write exact results and any canon deviations to `IMPLEMENTATION-NOTES.md`.
