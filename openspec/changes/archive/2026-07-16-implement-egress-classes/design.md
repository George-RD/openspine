# Design: Egress classes

## Authority boundary

AD-060 is enforced at the kernel gate boundary. The endpoint class is never accepted from `ActionRequest`: a shell can omit or spoof request fields, so request-carried classification would fail open. `gate()` takes a required `EgressClassifier` trait object. Every caller must provide one at compile time; the kernel's `ConnectorRegistry` is the production implementation. Test-only unrated paths explicitly pass `NoEgress`.

The classifier maps `ActionId` to an optional `EgressClass`. `None` means the action is not a registered egress endpoint. A rated endpoint is fail-closed against the grant: for shell-origin requests, the gate first honors an explicit deny, then requires the class to appear in `TaskGrant.allowed_egress_classes` before it considers approval or allow lists; kernel-origin requests apply the same class check before trusted-origin resolution. This preserves explicit-deny precedence while preventing rated endpoint restrictions from being bypassed.

## Registry ratings

`ConnectorRegistry` owns a `HashMap<ActionId, EgressClass>`. It is initialized with the three AD-060 web endpoints:

- `web.search` → `Search`
- `web.forum_browse` → `ForumBrowse`
- `web.form_submit` → `WebFormPost`

The `Connector` trait also exposes an endpoint-rating seam for future concrete connectors. Registration is immutable by key: duplicate same-class declarations during constructor aggregation are idempotent, while a conflicting class returns `EgressRegistrationError` and aborts construction. A connector cannot downgrade a built-in rating.

## Grant and MAC model

`CapabilityPack.allowed_egress_classes` is copied into the live `TaskGrant` during authority composition. It is not inferred from action ids and does not broaden candidate action lists. `RootAuthority` includes the class list in its canonical MAC payload, sorted by stable kebab-case identifiers. Mutating the list after sealing therefore invalidates `verify_mac`, preserving D-007 and D-011's digest/authority integrity.

The class check occurs after chain validity, expiry, catalog membership, and shell explicit-deny, but before kernel-origin classification, selection-token validation, approval-required, and ordinary allow resolution. This makes an uncovered class a structural policy denial rather than an approval prompt or trusted-origin bypass, while explicit shell denies remain the higher-precedence policy result.

## Compatibility

The new grant and pack fields use `serde(default)` for old serialized artifacts. Existing unrated Telegram/Gmail actions remain behaviorally unchanged because the registry returns `None` for them. The canonical catalog includes the three registered web endpoint action ids, while dispatch implementation remains out of scope.

## Alternatives rejected

1. **Put `egress_class` on `ActionRequest`:** rejected because shell-controlled omission/spoofing would bypass the check.
2. **Optional classifier/default `None` in `GateContext`:** rejected because a future caller could pass `Store` directly and silently bypass enforcement.
3. **Blind map overwrite:** rejected because a connector or later registration could downgrade a side-effecting endpoint to a less restrictive class.
4. **Encode class in `ActionId`:** rejected because AD-060 requires the connector registry to remain the endpoint-rating source of truth, separate from action-universe membership.
