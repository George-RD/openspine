# direct-terminal-chat Specification Delta

## MODIFIED Requirements

### Requirement: Local terminal messages SHALL use the governed owner pipeline

A message entered through `openspine chat` SHALL become a kernel-minted, locally verified owner event and traverse deterministic routing, authority composition, signed task-grant persistence, contained shell execution, model gateway mediation, action gating, and audit before a reply is displayed.

#### Scenario: Owner sends a freeform terminal message

- **WHEN** the local operator enters a non-empty message in `openspine chat`
- **THEN** OpenSpine creates a `cli.owner.message` event verified by `local_cli_auth`
- **AND** resolves the configured owner principal on an owner-device channel
- **AND** runs the terminal assistant under a persisted task grant
- **AND** displays only a reply delivered through `terminal.reply:owner_device`.

## ADDED Requirements

### Requirement: The terminal surface SHALL present owner review as an authenticated presentation/input adapter

The terminal SHALL render the single stored channel-neutral owner-review object and SHALL submit principal-bound decision intents (Approve, Reject, Narrow, Edit, Pause, Resume, Expire, Revoke, Inspect) against the same binding digest. The terminal adapter MUST NOT add scope, decisions, or lifecycle logic absent from the stored object, and MUST NOT synthesize a Telegram chat id to reuse the lifecycle. The terminal SHALL authenticate the owner through the local-owner envelope (`Source::Cli` + `LocalCliAuth`) and bind the resolved owner principal onto every decision.

#### Scenario: Terminal submits a review decision

- **WHEN** the owner issues a terminal command to decide a pending review
- **THEN** the kernel MUST authenticate the decision to the owner principal via the local-owner envelope
- **AND** bind the decision to the stored review's binding digest
- **AND** record the decision against the same stored review object a Telegram decision would record.

#### Scenario: Terminal must not fake a Telegram chat id

- **WHEN** the terminal surface submits a review decision
- **THEN** it MUST NOT synthesize or reuse a Telegram chat id to reach the lifecycle
- **AND** the decision MUST be recorded against the terminal's own owner-surface reference.

#### Scenario: Terminal cannot add scope or decisions

- **WHEN** a terminal command names a decision or a reviewed-scope value absent from the stored review object
- **THEN** the kernel MUST refuse the intent
- **AND** the refusal MUST be audit-recorded.

#### Scenario: Terminal cannot mutate lifecycle state

- **WHEN** a terminal command attempts to change a review or standing-rule lifecycle state directly
- **THEN** the kernel MUST refuse the mutation
- **AND** the refusal MUST be audit-recorded
- **AND** no lifecycle state MUST change.

#### Scenario: Terminal cannot re-derive authority

- **WHEN** a terminal command attempts to mint, widen, or re-derive a grant or authority from the review object
- **THEN** the kernel MUST refuse the attempt
- **AND** no grant or authority MUST be created or widened.

#### Scenario: A non-owner terminal principal submits a decision

- **WHEN** a terminal session that is not the bound owner principal submits a decision intent
- **THEN** the kernel MUST refuse the decision
- **AND** the review state MUST NOT change
- **AND** the refusal MUST be audit-recorded with a principal-mismatch reason.

#### Scenario: Terminal rejects a pending review

- **WHEN** the owner issues a terminal Reject for a pending review
- **THEN** the review MUST transition to `Rejected`
- **AND** no effect MAY run under the rejected review.

#### Scenario: Terminal inspects a review without mutating it

- **WHEN** the owner issues a terminal Inspect for a review
- **THEN** the kernel MUST return the stored review object
- **AND** the review state MUST NOT change
- **AND** no state-transition audit event MUST be written.

### Requirement: Owner decision code SHALL use a typed owner-surface reference, not a naked chat integer

Generic review, decision, pending-action, notification, and receipt code SHALL reference the owner via a typed `OwnerSurfaceRef` that is principal-bound, and SHALL NOT accept a naked `bound_chat_id: i64`. Connector-specific rendering ids MAY remain inside adapter storage, but generic code MUST NOT consume them. The surface reference SHALL represent at least a verified Telegram private owner chat and an authenticated local terminal/device session, and it SHALL be minted only by the adapter that authenticated the surface.

A grant's bound surface SHALL be persisted alongside the grant and SHALL be compared as a whole value wherever channel binding is enforced, so a decision arriving on a different surface — including a different channel with the same principal — is refused.

**Residual, stated deliberately rather than implied away:** the persisted surface is a sibling column (`task_grants.owner_surface_json`) and is NOT inside the grant's MAC envelope, which covers `grant_json` only. An actor with direct write access to the kernel database can therefore repoint a grant's reply surface without invalidating its MAC. This is within the existing trust boundary for that column set — the same actor can rewrite `grant_json`'s peer columns — but it is strictly weaker than MAC coverage and MUST NOT be described as authenticated. Closing it means either moving the surface inside the sealed grant or extending the MAC over the column, and is deliberately out of scope here.

#### Scenario: Generic decision code consumes a surface reference

- **WHEN** review or lifecycle code records a decision
- **THEN** it SHALL use the typed owner-surface reference bound to the principal
- **AND** it MUST NOT consume a naked chat integer from generic code.

#### Scenario: Existing Telegram grants remain valid

- **WHEN** the surface-reference seam is introduced
- **THEN** existing Telegram grants and pending rows MUST remain valid or fail closed
- **AND** a migration/compatibility test MUST prove they are not silently reinterpreted.
