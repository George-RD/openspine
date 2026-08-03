# Change: Define the protocol-neutral responsibility contract

## Layer

OpenSpine core and the Lyra owner-review product surface.

## Authority sensitivity

Authority-sensitive. This change defines which trusted context may be reviewed for reusable delegation, but it does not activate any new authority or alter current Gmail behavior.

## Dependencies

- D-007: the task grant remains the only live runtime authority object.
- D-053/D-103: action existence and egress/output classifications are kernel-owned catalog data.
- D-107: standing rules remain bounded, reviewed composition inputs.
- Roadmap parent #123 and contract issue #126.

## Why

OpenSpine can persist bounded standing rules, but it does not yet have a protocol-neutral contract that explains the job being delegated, proves the exact trusted context the owner reviewed, distinguishes evidence classes, or detects when connector/account/workflow drift invalidates that review.

Without that contract, progressive delegation risks becoming a Gmail-specific wrapper around one action id. It could also overstate weak evidence as a learned pattern, hide a connector implementation change behind an unchanged action name, or treat a standing rule as a second authority object.

## What Changes

- Add independent action-semantics and action-implementation descriptor axes to the canonical action catalog.
- Add a sealed `ResolvedActionContext` produced from kernel-resolved connector, account, target, counterparty, workflow, and task-shape inputs.
- Derive a versioned `ReviewedActionScope` from declared generic dimensions, with no protocol-name branches.
- Define distinct delegation evidence classes and digest a complete repeated-approval evidence set.
- Define one channel-neutral, digest-bound `OwnerReviewRequest` semantic object.
- Define `ResponsibilityManifest` as a reference/view over reviewed workflow and standing-rule artifacts, never as live authority.
- Fail closed on missing descriptor, implementation, resolver, executor, required scope, connector/account resolution, or compatibility version.
- Forbid dark-window Allow defaults for communication and connector-write effects in this contract.
- Register the semantics of `email.create_draft` while deliberately leaving its reusable implementation unresolved until #127 provides the shared real executor/resolver.

## Acceptance Criteria

- A synthetic non-Gmail implementation can derive and compare a reviewed scope without protocol branches.
- Changed connector instance, account identity, target, workflow, policy version, or workflow version produces mismatch or `needs_review` evidence.
- Unresolved counterparties and missing required scope dimensions fail before owner review.
- Repeated-approval evidence requires at least two distinct principal-authenticated owner decision records and binds the complete evidence set.
- The owner-review contract round-trips through strict schemas and contains no Telegram/terminal transport fields.
- The responsibility manifest contains references, scope, limits, compatibility, provenance, and controls, but no task-grant or allowed-action authority fields.
- The existing Gmail selected-thread and approved-draft paths remain behaviorally unchanged.
- `./scripts/check.sh define-responsibility-contract` passes before archive; `./scripts/check.sh` passes after archive.

## Out of Scope

- Executing a reusable delegated email draft; owned by #127.
- Matching and accumulating live evidence; owned by #128.
- Rendering/submitting owner review over Telegram or terminal; owned by #129.
- Responsibility persistence, pause/resume/revoke handlers, and the first end-to-end recurring draft proof; owned by #130 and later roadmap slices.
- Enabling dark-window Allow for any communication effect.
