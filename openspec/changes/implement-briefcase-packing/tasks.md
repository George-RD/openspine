# Tasks

- [x] Add deterministic schema briefcase and visibility primitives
- [x] Bind top-up decisions to source digest
- [x] Reject duplicate top-up request IDs
- [x] Add kernel Grant→Run briefcase packing boundary
- [x] Persist briefcases and worker visibility records
- [x] Add atomic persisted top-up mediation
- [x] Add determinism and visibility tests
- [x] Validate full local quality gates
- [x] Bounded metadata-only Gmail recipient read at response boundary
- [x] Persist opaque digest counterparty ref (no plaintext address at rest)
- [x] Cap and digest top-up justification (no plaintext at rest)
- [x] Audit kind derives from outcome (applied vs denied)
- [x] Selection token atomic with grant+briefcase (rollback on failure)
- [x] Structured GmailError (no provider body persisted/audited)
- [x] Split modules under 500-line limit
- [x] Same-pool stranger-vs-owner preference+skill depth test (`same_pool_stranger_and_owner_pack_preference_and_skill_depth`)
- [x] Injected pack/persist failure asserts no spawn and no orphaned token/grant (`injected_briefcase_persist_failure_leaves_no_spawn_or_orphans`)
- [x] Defer GET /v1/briefcase integration visibility-record test to worker-runtime (schema negative guard covered here)
