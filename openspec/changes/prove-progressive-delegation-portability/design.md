## Context

The canon for this change is D-146 (protocol-neutral two-axis responsibility contract) and the brief at `openspec/openspine-change-sequence.md:668-682`. The brief's invariant is the whole point: "adding a connector supplies reviewed adapters and fixtures; it cannot self-authorize delegation or require protocol branches in generic matching, evaluation, review, or lifecycle code."

Two design questions are settled before authoring, so they are not reopened during implementation.

## Decision 1 — the second shape is a deterministic in-repo test connector modelling a workspace message

The brief explicitly permits a deterministic test connector. That is the right choice here, not a shortcut: an external workspace integration would add a credential, an OAuth flow, and a network dependency, none of which the portability claim needs. What the claim needs is a shape whose *semantics* differ, and a deterministic connector can supply that.

The shape is a shared-workspace message. It differs from `email.create_draft` on four axes that the generic engine actually reads:

| Axis | `email.create_draft` | second shape |
| --- | --- | --- |
| `EffectKind` | `OwnerAccountWrite` | `SharedWorkspaceWrite` |
| `DataDestination` | `OwnerCloudAccount` | `SharedWorkspace` |
| Visibility model | one thread, one reply recipient | channel vs direct message, plus a participant set |
| Target semantics | mail thread reference | channel or conversation reference |

`EffectKind::SharedWorkspaceWrite` and `DataDestination::SharedWorkspace` already exist in `crates/openspine-schemas/src/delegation_contract.rs`; they are not being invented for this change. `DataDestination::SharedWorkspace` is inside `is_communication_or_connector_write`, so the new shape is subject to the existing communication dark-window prohibition and must declare `DarkWindowPolicy::Prohibited`. That is a real conformance point, not boilerplate: it is the first shape other than email to exercise it.

The shape is **not** counterparty-facing. D-057's counterparty-facing set is explicit and closed at `email.send`, and widening it is a separate decision with its own denial and escalation consequences. A shared-workspace write is classified by its own effect kind, so the portability proof needs no authority widening — which is exactly what the brief's "cannot self-authorize delegation" invariant demands.

Renaming Gmail fields would prove nothing. The connector must model visibility: a message addressed to a channel is visible to that channel's members, a direct message is not, and a participant joining a reviewed channel changes who can see the next effect.

## Decision 2 — visibility is expressed through the existing generic scope dimensions

`ReviewedScopeDimension` (`crates/openspine-schemas/src/delegation_contract.rs:100-119`) already carries `EffectDestination`, `OutputChannel`, `Target`, `TargetDigest`, and `BoundParameters`. The binding is:

- **`EffectDestination`** — workspace-visible versus direct message. This is the coarse visibility class.
- **`OutputChannel`** — which channel the effect lands in.
- **`BoundParameters`** — the kernel-resolved participant or member set, so a new participant in a reviewed channel stops the rule matching.

Adding a `ReviewedScopeDimension::Visibility` variant is precisely the protocol-specific branch the brief's invariant forbids: it would encode one protocol's concept in the generic comparison type and every later shape would want its own variant. The existing dimensions carry the semantics without that.

This mirrors #130's `Target` / `TargetDigest` / `BoundParameters` split for Gmail, where `Target` was the thread reference, `TargetDigest` sealed the single reply address, and `BoundParameters` carried the participant set. The same three-way split expresses workspace visibility, which is evidence that the generic dimensions were the right abstraction rather than a coincidence.

**Open question, deliberately not decided here.** If implementation proves a genuine visibility semantic that none of `EffectDestination`, `OutputChannel`, `Target`, `TargetDigest`, or `BoundParameters` can express — a plausible candidate is a thread-within-channel reply whose visibility differs from both the channel and a DM — that gap is recorded here as an open question and returned to the owner. It is **not** resolved by adding a dimension variant mid-implementation.

## Catalog wiring

The Gmail entries in `crates/openspine-kernel/src/action_catalog_data.rs` are the pattern to copy:

- `delegation_descriptors()` (`:19`) — semantics, `reusable_delegation: true`, `required_scope_dimensions`, and `delegation_policy` bounds with quota/rate windows, lapse, proposal mode, defaults, `dark_window_policy`, and `fresh_target_selection_required`. The new shape's required dimensions must include `EffectDestination`, `OutputChannel`, and `BoundParameters` alongside the connector, account, target, counterparty, relationship-tier, workflow, and task-shape dimensions.
- `implementation_descriptors()` (`:93`) — its own `implementation_id`, `connector_kind`, `executor_id`, `resolver_id`, and explicit versions.
- `egress_declarations()` (`:109`) — an explicit entry. `None`/`None` is a deliberate non-egress classification, never a default: an action absent from the table fails closed at the gate. A workspace post is none of `EgressClass::{Search, ForumBrowse, WebFormPost}`, so `None`/`None` is the honest classification, written literally.

## Sequencing and non-goals

Generic code changes should be *data and registration only*. If implementation finds itself adding a parameter, enum variant, or conditional to reviewed-scope comparison, evidence construction, evaluation, owner review, receipt assembly, or lifecycle controls, that is the invariant failing and is a signal to stop and re-read this document rather than to add the branch.

Connector and task-shape abstraction generalization is #132's job; this change is the design evidence #132 consumes, so it deliberately leaves the abstractions where they are.
