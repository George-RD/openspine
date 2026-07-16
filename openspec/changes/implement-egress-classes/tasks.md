# Tasks: Implement egress classes

- [x] Add the shared `EgressClass` enum and stable serde identifiers.
- [x] Add egress-class references to capability packs and task grants with serde defaults.
- [x] Include grant egress classes in the grant-chain root MAC payload.
- [x] Add structured `EgressClassNotGranted` denial vocabulary.
- [x] Add required `EgressClassifier` parameter to `gate()` and enforce rated classes before approval/allow resolution.
- [x] Add connector-registry endpoint ratings for search, forum browse, and web-form POST.
- [x] Reject conflicting endpoint-class registrations without changing existing ratings.
- [x] Propagate pack classes through authority composition.
- [x] Add the AD-060 web endpoint action ids to the canonical catalog.
- [x] Wire every production gate call through `ConnectorRegistry` as classifier.
- [x] Test registry ratings and conflict immutability.
- [x] Test pack-to-grant propagation.
- [x] Test grant MAC invalidation after egress-class tampering.
- [x] Test search-class grant denies web-form POST and allows search.
- [ ] Add a decision-log entry — intentionally skipped: implementation preserves settled AD-060 and introduces no canon deviation.
- [ ] Implement real web connector dispatch — intentionally skipped: explicitly out of scope for this change.
