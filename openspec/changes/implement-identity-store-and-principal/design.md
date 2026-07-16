# Design: Implement identity store and principal

## Approach

Four pieces, each minimal and authority-narrowing: a `Principal` schema, a
persisted identity store with a DB-level single-owner invariant, a read-only
`IdentityResolver` seam, and the composition cutover to `principal_id`. New
kernel code lands in new files (`store/identity.rs`, `identity.rs`) to keep
every file under the 500-line cap; `pipeline/mod.rs` loses the hardcoded
`resolve_owner_identity` rather than gaining a wrapper.

### 1. Principal schema — authority-free, single-owner-shaped

- `crates/openspine-schemas/src/principal.rs`: `Principal { id: Ulid,
  identity_id: Ulid, is_owner: bool, schema_version: u32 }`, `deny_unknown_fields`.
  No capability/route/grant/tool field — D-006 extended to the principal. v1 has
  exactly one row with `is_owner == true`; the field is the promotion seam
  (additive later), never authority.
- `IdentityResolution` gains `principal_id: Option<Ulid>`. `Option` is the
  whole point: only the owner fast path sets `Some`; a bound counterparty and
  an unknown both leave it `None`, so resolving/binding an identity can never
  implicitly mint a principal. `deny_unknown_fields` makes the field's presence
  a schema-level fact.

### 2. Identity store — DB-enforced single owner, audited binding

- Tables added to `store/mod.rs` `SCHEMA_SQL` (all `CREATE ... IF NOT EXISTS`):
  `principals(id, identity_id, is_owner, principal_json)`,
  `identities(id, identity_json)`, and `identity_identifiers(value_hash,
  identifier_kind, identity_id)` for resolution lookup.
- **Single-owner invariant is a database fact, not an application check.** A
  partial unique index `CREATE UNIQUE INDEX idx_principal_owner_singleton ON
  principals(is_owner) WHERE is_owner = 1` makes a second owner insert fail at
  the SQLite layer regardless of caller or concurrency. `bootstrap_owner_principal`
  is transactional and idempotent: read existing owner → if absent, insert → on
  unique-constraint violation, re-read the winner. A test asserts a second owner
  insert is rejected.
- `store/identity.rs` (new module, registered) holds: `bootstrap_owner_principal`,
  `owner_principal`, the owner-verified `owner_principal_by_id`,
  `owner_assert_identity_binding`, `resolve_identity_by_identifier_hash`, and
  test helpers. Binding appends an `identity.bound` audit row recording the
  asserting owner principal; the identifier value is stored only as its hash
  (D-012 posture: raw values never persisted).

### 3. IdentityResolver — read-only seam, owner fast path

- `crates/openspine-kernel/src/identity.rs` (new): `IdentityResolver` borrows
  the store plus the bootstrapped owner principal id and the owner's identifier
  hash. `resolve` is **pure of side effects** — it never inserts a row.
  - Owner fast path: if the caller presents `owner_verified = Some(&VerifiedOwnerContext)` (where
    `VerifiedOwnerContext` is an unforgeable connector-authenticated token defined inside `telegram.rs`
    that only verification code can construct) → `principal_id = Some(owner)`, confidence `1.0`.
  - Counterparty: hash lookup in `identity_identifiers` → returns the bound
    `Identity` with its relationship, `principal_id = None`.
  - Unknown: no match → `RelationshipKind::Unknown`, confidence `0`,
    `principal_id = None`, and crucially **no row is written**.
- The old free function `resolve_owner_identity` in `pipeline/mod.rs` is
  deleted; the identify stage calls the resolver.

### 4. Composition cutover — principal_id, fail closed

- `AuthorityInput.user: &'a str` → `principal_id: Ulid`.
  `compose_authority` sets `TaskGrant.user = input.principal_id.to_string()`.
  `TaskGrant.user` keeps its name and remains the grant-chain identity field
  (CLAIM-26); its value is now the resolved principal id, not the Telegram id.
- `driver.rs` identify stage: `let principal_id = identity.principal_id
  .ok_or_else(|| anyhow!("no principal resolved"))?;` then compose. A pipeline
  event that resolves no principal fails closed — strictly narrower than today's
  unconditional config-string read.
- `AppState` keeps `owner_user_id: i64` (the Telegram *auth* signal for
  `verify_update`, selection-token, and approval attribution — a channel
  credential, distinct from the composition identity) and gains
  `owner_principal_id: Ulid`, bootstrapped once at kernel startup. The owner
  config string is no longer read for composition.

### 5. Owner-asserted binding — audited, owner-context-gated, agent-unreachable

- Binding is an owner-control operation, **not** a gate-mediated agent action:
  it is a kernel-internal store mutation (like `insert_task_grant`), so D-004's
  "every effect through `gate()`" (an agent-effect rule) is not the relevant
  gate; the relevant guarantee is the owner-principal context.
- `owner_assert_identity_binding(owner_principal_id, proof, identity)` requires the passed id to resolve to
  an `is_owner` principal AND a valid `VerifiedOwnerContext` proof token at the boundary — this is the owner-approval
  proof at the API boundary, **not** `with_kernel_origin` and **not** pack exclusion (those are defense-in-depth only).
  It is exposed only from the owner-control lane, which runs after `verify_update` authenticated the owner.
- It is unreachable from the agent path: absent from the action catalog, every
  capability pack, and the shell `ActionHandlerRegistry`; the shell dispatches
  only allowed actions, so a request for it cannot be served.

## Key decisions

- **`principal_id` is `Option` on the resolution.** AD-146: a counterparty is
  not a principal. An unconditional `Ulid` would let any resolution promote an
  identity to a principal. `None` for non-owners makes "binding ≠ promotion" a
  structural fact and leaves multi-principal additive.
- **Single owner is a partial unique index, not a count check.** A runtime
  `count` guard is bypassable under concurrency or a second bootstrap path; the
  DB constraint is the invariant, and the bootstrap is idempotent against it.
- **`TaskGrant.user` is not renamed.** It is a persisted, grant-chain-MAC'd
  field (CLAIM-26). Renaming would churn the MAC preimage and ~10 fixtures for
  no authority gain; changing its *value* to the principal id is the
  load-bearing cut.
- **Binding is owner-context-gated, not kernel-origin-gated.** `with_kernel_origin`
  proves the caller is kernel code, not that the owner approved this binding.
  The owner-principal context (only constructible post-`verify_update`) plus the
  `is_owner` assertion at the API boundary is the owner-approval guarantee.
- **Owner Telegram id stays as the auth signal.** Channel authentication
  (`verify_update`) is a separate concern from composition identity; conflating
  them is exactly what AD-146 unwinds for composition.

## Alternatives considered

- **`principal_id: Ulid` (non-optional) on `IdentityResolution`:** rejected —
  it implies every resolution yields a principal and lets a bound counterparty
  be promoted by resolution alone, violating AD-146.
- **Binding as a `with_kernel_origin` gate action:** rejected as the
  authorization proof — kernel-origin only proves kernel code, not owner
  approval (it stays usable as defense-in-depth, never as the guarantee).
- **Rename `TaskGrant.user` → `principal_id`:** rejected — persisted MAC'd
  field; value change is sufficient and far lower risk.
- **`PRAGMA user_version` migrations for the new tables:** deferred — the store
  has no `user_version` mechanism yet (`store/mod.rs` doc); the new tables use
  `CREATE ... IF NOT EXISTS` matching every existing table, and
  `implement-day2-operations` owns the migration substrate. No existing
  on-disk table is altered.

## What does NOT move

- Grant chain MAC construction and fields; gate semantics and call sites; the
  audit chain. Selection-token `user` and approval `approved_by` keep using the
  owner Telegram id (owner attribution records, not authority composition).
