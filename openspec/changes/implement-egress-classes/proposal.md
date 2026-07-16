# Proposal: Implement egress classes

## Dependencies

- `refactor-kernel-registries` (archived): provides the kernel `ConnectorRegistry` registration seam.
- AD-060 (settled): egress endpoints are typed and policy-rated in the connector registry; packs reference egress classes.
- Kernel invariants D-004, D-005, D-006, D-007, D-008, D-010, and D-011 remain binding.

## Problem/Context

The connector registry currently enumerates connector instances but does not classify the external endpoints they expose. A grant can therefore distinguish action ids but cannot express the security-relevant difference between a read-only search query and a side-effecting web-form submission. The shell must not be trusted to declare an endpoint's class: omission or spoofing of request metadata would create a gate fail-open path.

## Proposed Solution

Introduce a closed `EgressClass` enum (`search`, `forum-browse`, `web-form-post`) in the shared schemas. The connector registry owns immutable endpoint-to-class ratings, including the AD-060 web endpoint registrations, and implements a required gate classifier interface. Conflicting registrations are rejected without changing the original rating.

Capability packs reference allowed egress classes. Authority composition copies those classes into the live task grant, and the grant-chain root MAC covers them. `gate()` receives a required trusted classifier, resolves the requested action's class from the registry rather than from shell-provided request data, and returns structured `EgressClassNotGranted` when the grant does not cover it.

## Acceptance Criteria

- The connector registry returns `Search`, `ForumBrowse`, and `WebFormPost` for the corresponding registered web endpoints.
- A conflicting class registration is rejected and cannot downgrade an existing endpoint rating.
- Capability-pack egress classes propagate into the composed task grant.
- Grant-chain MAC verification fails after an egress-class list is mutated.
- `gate()` requires a classifier and denies a `web.form_submit` request when the grant only allows `Search`, with `EgressClassNotGranted`.
- A `web.search` request with the same search-only grant is allowed.
- `cargo fmt`, clippy with `-D warnings`, workspace tests, file-size checks, and strict OpenSpec validation pass.

## Out of Scope

- Implementing a web connector or performing real HTTP search, forum, or form operations.
- Adding dynamic or shell-controlled endpoint registration.
- Connector health/circuit-breaker behavior from AD-103.
- Changing existing Telegram/Gmail action semantics or grant precedence outside the egress-class constraint.
- Adding a decision-log entry: no settled canon is narrowed or reversed.
