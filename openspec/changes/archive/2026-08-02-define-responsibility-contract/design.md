# Design

## Trust boundary

The action catalog owns two independent readiness axes:

1. `ActionDescriptor` defines owner-facing semantics, effect class, reversibility, destination, reusable-delegation eligibility, required reviewed-scope dimensions, and policy bounds.
2. `ActionImplementationDescriptor` defines one concrete connector implementation plus resolver/executor identities and versions.

A proposal may proceed only when the canonical action exists, both descriptors exist, their action ids agree, every required field is complete, and the pure delegation validator succeeds. An action name alone is never sufficient.

`email.create_draft` receives its semantic descriptor in this change. It intentionally has no reusable implementation descriptor yet. That makes the roadmap state honest: the product knows what the job means, but cannot claim the reusable real effect path exists before #127.

## Resolved context and reviewed scope

The kernel resolves connector instance, account role/identity, canonical targets, bound counterparty identity, relationship tier, kernel-bound parameters, digests, effect classifications, workflow, and task shape. The resulting `ResolvedActionContext` has private fields and is constructed only through a validating function.

`ReviewedActionScope` stores a generic map from declared `ReviewedScopeDimension` values to typed values. Scope derivation and comparison switch on those dimensions only. They contain no Gmail, Matrix, Telegram, Slack, or other protocol branch.

The scope always binds action id and descriptor version. Connector implementation values bind implementation id/version, connector kind, and resolver/executor ids and versions. A change on any reviewed dimension produces a typed mismatch set instead of silently widening.

## Evidence and proposal truthfulness

`DelegationEvidence` distinguishes repeated approvals, explicit owner requests, correction/workflow proposals, and manually supplied artifacts. Only repeated approvals support a pattern claim. The repeated-approval constructor requires at least two unique decision events from one owner principal and one request class, sorts them deterministically, and digests the complete evidence set.

Evidence is provenance, not authority. It may justify an owner-facing proposal but cannot activate a standing rule or responsibility.

## Owner review

`OwnerReviewRequest` is the semantic object rendered by any authenticated owner surface. It includes proposal provenance, exact reviewed scope, automatic effects, remaining approval boundaries, quota/rate/expiry, fail-closed fallback behavior, proposal and compatibility digests, decisions, and lifecycle controls.

The contract contains no chat id, callback id, Telegram method, terminal command, or other transport field. A binding digest covers the complete semantic review object so a channel adapter cannot submit a decision against altered scope or limits.

## Responsibility is a reference view

`ResponsibilityManifest` references a workflow id and standing-rule id and records reviewed scope, limits, compatibility versions, provenance, status, and lifecycle controls. It deliberately contains no task grant, action allowlist, capability pack, or direct executor authority.

At runtime, each task still passes through ordinary routing, authority composition, task-grant minting, and `gate()`. The responsibility is legibility and lifecycle state over existing artifacts, not a second grant system.

## Drift and removal

Compatibility assessment is deterministic:

- missing resolved context, including connector/account removal, becomes `needs_review`;
- reviewed-scope mismatch becomes `needs_review`;
- descriptor, implementation, policy, or workflow version drift becomes `needs_review`.

The contract never remaps an unavailable connector or reconfigured account to a successor implicitly. Re-review is required.

## Dark windows

Communication and connector-write effects may use only `Prohibited` or bounded deny-only policy at this layer. `BoundedAllow` is structurally rejected. A future change may revisit this only with a new explicit decision and adversarial proof.

## Existing behavior

No production dispatch, matcher, persistence, approval callback, Gmail connector, or standing-rule mediation path consumes the new contract yet. Current selected-thread reading and approved draft creation therefore remain unchanged.
