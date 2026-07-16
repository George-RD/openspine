# Tasks: Implement identity store and principal

## 1. Schemas

- [x] `crates/openspine-schemas/src/principal.rs`: `Principal { id, identity_id,
  is_owner, schema_version }`, `deny_unknown_fields`, no authority field.
- [x] Register `pub mod principal;` in `openspine-schemas/src/lib.rs`.
- [x] `IdentityResolution` gains `principal_id: Option<Ulid>` (Some only for the
  owner fast path); update its doc and the in-crate test.

## 2. Identity store

- [x] `SCHEMA_SQL` (`store/mod.rs`): add `principals`, `identities`,
  `identity_identifiers` tables and the partial unique index
  `idx_principal_owner_singleton ON principals(is_owner) WHERE is_owner = 1`.
- [x] `store/identity.rs` (new module, registered): `bootstrap_owner_principal`
  (transactional/idempotent, unique-violation-tolerant), `owner_principal`,
  `owner_principal_by_id` (asserts `is_owner`), `owner_assert_identity_binding`
  (audits `identity.bound` with the asserting owner; stores only identifier
  hashes), `resolve_identity_by_identifier_hash`, test helpers. Unique-violation
  mapped off `rusqlite` constraint errors.

## 3. IdentityResolver seam

- [x] `crates/openspine-kernel/src/identity.rs` (new, registered): read-only
  `IdentityResolver` — owner fast path (`Some` principal), counterparty lookup
  (identity, `None` principal), unknown (`Unknown`, confidence 0, `None`, no
  write). Delete `resolve_owner_identity` from `pipeline/mod.rs`.

## 4. Composition cutover + bootstrap

- [x] `AuthorityInput.user: &'a str` → `principal_id: Ulid`; `compose_authority`
  sets `TaskGrant.user = input.principal_id.to_string()`; update the doc.
- [x] `driver.rs` identify stage uses the resolver; compose stage takes
  `identity.principal_id` and fails closed when `None`.
- [x] `AppState` gains `owner_principal_id: Ulid`; kernel startup bootstraps the
  owner principal + owner identity from config (`owner_user_id`) before serving.
- [x] Update `openspine-authority` compose tests + kernel `AppState`/test
  fixtures for the `principal_id` input.

## 5. Owner-asserted binding path

- [x] Wire the audited binding from the owner-control lane (post-`verify_update`)
  using the owner-principal context; confirm it is absent from the action
  catalog, every capability pack, and the shell `ActionHandlerRegistry`.

## 6. Tests

- [x] Owner fast path yields `principal_id`; counterparty resolves to identity
  with `principal_id = None`; unknown → `Unknown`, confidence 0, no binding row.
- [x] Second owner principal insert is rejected (DB invariant).
- [x] Owner can assert a binding (audited as `identity.bound`); a binding call
  without the owner-principal context is rejected; the resolver/agent path
  creates no binding.
- [x] Composition consumes `principal_id` (grant `user` holds the principal id,
  not the Telegram id); no-principal resolution fails closed.
- [x] `Identity` and `Principal` carry zero authority fields (D-006 structural
  assertion).

## 7. Decision log + claims + docs

- [x] Add threat-claim rows (owner-only binding; unknown-never-binds;
  single-owner DB invariant) mapped to real test names in `docs/threat-claims.md`.
- [x] Add a D-0XX decision-log entry only if implementation narrows/drops canon
  (none expected: single-owner v1 is in-scope AD-146, not a narrowing).
- [x] `graphify update .` after code changes.

## 8. Validation

- [x] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`; `scripts/check-file-sizes.sh` (all ≤500 lines).
- [x] `openspec validate implement-identity-store-and-principal --strict` and
  `./scripts/check.sh` green.
- [x] Independent reviewer subagent pass on the full diff before commit
  (authority/spec-conformance lens, esp. D-006: no authority fields on
  `Identity`/`Principal`); wait for final yield; fix or justify every blocker;
  re-run `check.sh`.
