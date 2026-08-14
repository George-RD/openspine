## Why

`ship-recurring-gmail-draft-proof` (#130) proved the delegation path end to end for exactly one protocol. Every contract it exercised — descriptor and implementation declaration, kernel context resolution, reviewed-scope comparison, typed evidence, proposal-specific evaluation, channel-neutral owner review, activation, scoped admission, responsibility receipt, and lifecycle controls — is written as protocol-neutral, and none of it has ever run against a second communication shape. The archived #130 brief says so in its own invariant: "Gmail is the first vertical proof, not the generic architecture or evidence that other protocols work."

That gap is load-bearing rather than cosmetic. The capability map records `progressive-delegation` as `proof_in_progress` with the selected `recurring-gmail-draft` proof `shipped` and its `current_limit` reading "proven for one protocol only; portability across a second communication shape is unverified", and `scripts/capability-map.mjs` will not let a `second-protocol-portability` proof reach `verified` without non-empty `conformance_tests`. Until a materially different shape completes the same path, "protocol-neutral" is a claim about how the code is written, not a fact anyone has observed. #132 (`compose-whole-responsibilities`) additionally needs this change as design evidence before it finalizes connector and task-shape abstractions.

This change affects OpenSpine core runtime, the action catalog, kernel resolution and execution registries, and the capability-map portability evidence. It does not affect owner-facing surfaces, add an external credential, or widen any authority set.

## What Changes

- Add a second communication shape as catalog-owned data: one `ActionDescriptor` in `delegation_descriptors()` and one `ActionImplementationDescriptor` in `implementation_descriptors()` (`crates/openspine-kernel/src/action_catalog_data.rs`), plus a literal `egress_declarations()` entry. The shape is a shared-workspace message with `EffectKind::SharedWorkspaceWrite`, `DataDestination::SharedWorkspace`, and `DarkWindowPolicy::Prohibited`, so it differs from `email.create_draft` in effect kind, destination, reversibility, and visibility model rather than renaming its fields.
- Add a deterministic in-repo test connector for that shape, modelling workspace, direct-message, and channel visibility semantics, with its own kernel-side resolver and its own registered executor. No new external credential, OAuth flow, or network dependency.
- Express visibility through the existing generic scope dimensions only: `EffectDestination` binds workspace-versus-direct-message, `OutputChannel` binds the channel, and `BoundParameters` binds the kernel-resolved participant set. `ReviewedScopeDimension` is not extended.
- Drive the whole path for the new shape through the existing generic machinery: repeated-approval mining, proposal-specific evaluation, channel-neutral owner review with a digest-bound decision, owner-minted activation grant, scoped admission with exact-one selection, one real effect through the shape's executor, and a responsibility receipt.
- Add cross-shape, cross-account, cross-target, and cross-visibility confusion tests, plus the full fallback matrix for the new shape: erased counterparty, bound-context drift, exhausted quota or rate, pause, expiry, revocation, unresolved counterparty, evaluation staleness, and a fenced retry.
- Add the conformance tests that let the capability map's `second-protocol-portability` proof carry evidence, and record the reachability census required by D-172 for every guard the change adds.

- The change deliberately does **not** add a `ReviewedScopeDimension::Visibility` variant, a protocol discriminator on any generic function, a second authority object, an external workspace connector, or a counterparty-facing send for the new shape. It does not widen the catalog's non-delegable set, the counterparty-facing set (D-057, still `email.send` only), or the dark-window eligibility allowlist (D-162, still empty).

## Capabilities

### New Capabilities

None. This change proves existing protocol-neutral capabilities against a second shape rather than introducing a capability namespace.

### Modified Capabilities

- `responsibility-contract`: a second shape becomes delegable only by declaring its own descriptor, implementation, and literal egress row; visibility is expressed through existing generic dimensions; and the second shape must complete the whole path with no engine change.
- `gate-action-api`: the kernel-resolved-request guarantee holds per shape, one shape's executor is not reachable with another's resolved context, and an unregistered executor for a declared shape stays a typed `NoExecutor`.
- `standing-rules`: visibility values participate in exact-one selection, so cross-channel, cross-destination, and cross-participant-set confusion falls back to ordinary owner approval without moving budget.

## Dependencies

Requires archived #130 (`ship-recurring-gmail-draft-proof`) **HARD**, plus the archived #127, #128, #129, #133, and #135 contracts it builds on. No new external dependency.

## Out of Scope

- No counterparty-facing send for the new shape. Adding one would require amending D-057's closed counterparty-facing set and is its own decision.
- No external workspace connector, credential intake, or OAuth flow; the deterministic in-repo connector is the reviewed adapter.
- No new reviewed-scope dimension, and no generalization of connector or task-shape abstractions — that is #132, which consumes this change as design evidence.
- No change to the owner surfaces: the new shape renders through the existing channel-neutral review object.
- No claim that a third protocol works. This change makes portability an observed fact for two shapes, not a general proof.
