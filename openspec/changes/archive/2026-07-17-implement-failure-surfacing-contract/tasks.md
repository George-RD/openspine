# Tasks

- [x] Enumerate AD-137 audit append sites and record them in design.md.
- [x] Propagate action audit append failures instead of discarding them.
- [x] Implement truthful owner notification outcomes and dead-letter persistence.
- [x] Implement authority/escalation versus connector/resource routing.
- [x] Implement owner-retrievable digest and per-connector counters.
- [x] Add focused routing, digest, dead-letter, and counter tests, including updated atomic retry API coverage.
- [x] Require encrypted artifact persistence before dead-letter insertion; surface persistence failure in audit and digest without blank retries.
- [x] Document lossless technical digest pagination and at-least-once external delivery semantics.
- [x] Migrate digest detail to verified encrypted text_ref artifacts with fail-closed legacy sanitization.
- [x] Run the shared parent gate after review and landing (parent-owned).
