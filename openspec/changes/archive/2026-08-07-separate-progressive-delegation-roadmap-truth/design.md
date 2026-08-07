# Design: separate generic capability, selected proof, and portability evidence

## Which capability spec owns the checked roadmap contract

`openspec/specs/` has no spec named "capability-map" or "roadmap". The closest
existing owner is `openspec-development-process`: it is the spec for the
development/change-management layer, and it already governs the boundary
between OpenSpec (development) artifacts and OpenSpine runtime authority, the
requirement that completed changes are archived, and the requirement that a
proposal must not treat its artifacts as live authority. The checked capability
map and its generated public roadmap are exactly such a development/truth-
standard artifact: they translate archived changes and named tests into product
status and must not overclaim. Rather than invent a new capability spec, this
change **adds** the roadmap/capability-map truth requirements to the existing
`openspec-development-process` spec. This keeps the truth standard where the
development-process rules already live and avoids a parallel spec that would
compete with it for ownership of "how the repo reports status honestly."

No requirement in this delta already exists in the pre-seeded
`openspec/specs/openspine-development-process/spec.md` (it has no capability-map
or evidence-truth requirement today), so all delta requirements are `## ADDED
Requirements`. Each is written so that, once archived, it describes the *contract*
the capability-map tooling and validator enforce, not the tooling internals.

## The v2 schema shape

`capabilities/capability-map.json` moves from `schema_version: 1` to
`schema_version: 2`. The two top-level arrays are:

- `capabilities[]` — what a protocol-neutral or vertical capability is and
  whether it has landed. Each entry gains:
  - `generic` (boolean): true only for a protocol-neutral capability, never for
    a vertical proof;
  - `blocking_issues` (`[{ issue: integer, role: string }]`): issue references
    that explain blockers only, never runtime evidence. `role` is one of
    `architecture contract`, `execution/review foundations`, `scoped
    evidence/matching`, `proposal-specific evaluation`, or a future role.
  - a new `state` value `proof_in_progress` alongside `runtime_landed`,
    `product_surface_missing`, and `wired_into_lyra`;
  - each `owner_path_tests` entry may carry a `kind` of `positive_effect`,
    `fallback`, or `control`, so a generic wiring claim can be required to have
    both a positive-effect test and fallback/control coverage.
- `proofs[]` — replaces `starter_workflow_candidates[]`. Each proof:
  - `id`, `capability` (reference to a capability id), `kind` (`selected`,
    `portability`, `whole_responsibility`, or `candidate`), `selected`
    (boolean), `owner_outcome`, `reason`, `scope`, `current_limit`, `state`
    (`planned`, `in_progress`, `shipped`, `verified`), `tracking_issue`
    (integer, required when `kind` is selected/portability/whole_responsibility),
    optional `proof_sequence` (required for a selected proof, ≥ 4 steps),
  - evidence fields: `owner_path_tests` (for the selected proof),
    `conformance_tests` (for the portability proof). Both carry the same
    `{ path, test, kind }` shape and are checked against the repository.

`blocking_issues` and `tracking_issue` are **issue references**. The validator
rejects an integer where an evidence path is expected (e.g. an issue number in
`runtime_changes`), so issues can never be mistaken for landed proof.

Blocker **openness** is authorial, not machine-verified: the validator does not
call the GitHub API (a deterministic offline check must stay network-independent)
and therefore cannot itself confirm that a `blocking_issues` entry corresponds
to an issue still open. The validator checks the blocker's *shape* (`issue`
integer, known `role`) and that issues render as blockers rather than proof;
whether the issue is actually open is the author's responsibility when editing
the map. No requirement in this change's spec delta claims machine-verified
openness.

### Why a top-level `proofs[]` and not nested proof objects

Promoting proofs to a first-class top-level array whose entries reference a
capability id (CodeRabbit's chosen option 2) keeps the "exactly one selected"
invariant that already exists on `starter_workflow_candidates`, and lets
#130/#131/#132 advance their own evidence fields without touching the generic
capability identity. Nested proof objects would couple each proof's lifecycle
to its capability record and force a rewrite of the generic identity whenever a
proof's evidence changes.

### Why the stricter wiring rule applies only to generic capabilities

The ticket text scopes the "real positive effect + required fallback/control
cases" rule to a *generic* capability. The existing wired vertical capabilities
(`direct-terminal-conversation`, `selected-gmail-draft`) each have owner-path
tests that are not labeled by category. Applying the stricter categorized rule
to every wired capability would force relabeling existing tests and churn
already-valid vertical records. The generic capability is marked `generic:
true`, and the validator requires categorized `positive_effect` plus
`fallback`/`control` owner-path tests only for generic wired capabilities. The
existing "at least one named owner-path test" rule continues to apply to all
wired capabilities, including generic ones.

## Corrected current records

The `recurring-draft-responsibility` capability (the old "capability that is
also the proof") is replaced by a generic capability `progressive-delegation`:

- `id: progressive-delegation`, `generic: true`, `state: product_surface_missing`;
- protocol-neutral `owner_outcome`: "Let Lyra grow through real work by
  reviewing and delegating reusable, protocol-neutral responsibility for
  narrowly bounded recurring work.";
- `runtime_changes` / `canonical_specs` list the landed substrate confirmed in
  the `## Completed / archived` ledger section: standing rules
  (`implement-standing-rules` / `openspec/specs/standing-rules/spec.md`),
  reflection miner (`implement-reflection-miner` /
  `openspec/specs/reflection-miner/spec.md`), governed artifact lifecycle
  (`implement-artifact-lifecycle-slice` /
  `openspec/specs/artifact-lifecycle/spec.md`), audit artifact store
  (`backfill-implemented-capability-specs` →
  `openspec/specs/audit-artifact-store/spec.md`), and budget / spend kill
  switch (`harden-approval-and-budgets` and `implement-spend-kill-switch` →
  `openspec/specs/spend-kill-switch/spec.md`). Task grants and grant chain are
  represented by `define-grant-chain-and-modes` (the grant-chain brief stays in
  the ledger) and its `canonical_specs` entry
  `openspec/specs/authority-composition/spec.md`.
- `blocking_issues`: `#128` (`scoped evidence/matching`), `#129`
  (`execution/review foundations`), `#133` (`proposal-specific evaluation`).
  #126 and #127 are **archived** (CLOSED) substrate, not blockers — this
  corrects the stale CodeRabbit "missing prerequisites #126..#133" list, which
  was written before those two landed.

`selected-gmail-draft` remains a shipped vertical capability, distinct from the
generic capability.

`proofs[]`:

- `recurring-gmail-draft` — `kind: selected`, `selected: true`,
  `tracking_issue: 130`, `capability: progressive-delegation`, scope = one
  reviewed connector/account/relationship, the existing five-step
  `proof_sequence`, `state: planned`, evidence arrays empty (not yet shipped).
- `second-protocol-portability` — `kind: portability`, `tracking_issue: 131`,
  `state: planned`, `conformance_tests: []`.
- `whole-responsibility-progression` — `kind: whole_responsibility`,
  `tracking_issue: 132`, `state: planned`.
- `task-and-reminder`, `research-and-brief`, `skill-install-proof` —
  `kind: candidate`, `selected: false`, preserving the old unselected
  candidates.

## Validator truth rules (TDD)

`scripts/capability-map.mjs` `validateCapabilityMap` gains these rules, each
pinned by a failing test first:

1. schema_version must be `2`; `capabilities` non-empty; proofs non-empty.
2. generic/proof separation: a capability and a proof are distinct objects
   (proofs live in `proofs[]`, never duplicated as capabilities; a proof's
   `capability` reference must resolve to a known capability id).
3. exactly one proof has `selected: true`.
4. a proof with `state: shipped` requires non-empty `owner_path_tests`.
5. a generic capability with `state: wired_into_lyra` requires at least one
   `positive_effect` owner-path test and at least one `fallback` or `control`
   owner-path test — runtime changes alone cannot satisfy it.
6. a `kind: portability` proof with `state: verified` requires non-empty
   `conformance_tests` (second-protocol evidence).
7. a `kind: whole_responsibility` proof cannot be `verified` unless the
   selected proof is `shipped` and the portability proof is `verified`.
8. issue numbers are not evidence: an integer in `runtime_changes` (or any
   evidence array) fails; `blocking_issues`/`tracking_issue` are validated as
   issue references, never counted as proof.
9. every evidence path (`owner_path_tests`, `conformance_tests`, specs,
   artifacts, runtime changes) still resolves to a real file / archived change /
   registered Rust test, unchanged from v1.
10. the selected proof requires an integer `tracking_issue` and a
    `proof_sequence` of at least four steps.

## Migration

`scripts/capability-map-migration.mjs` exports `migrateCapabilityMap(map)`:

- if `schema_version === 2`, return the map unchanged (idempotent);
- else (version 1): set `schema_version: 2`; add `generic: false` and
  `blocking_issues: []` to every capability; rename `starter_workflow_candidates`
  to `proofs`; map each old candidate to a proof — the previously selected one
  becomes `kind: "selected"` with `selected: true` and its `task_boundary`
  moved into `scope`; unselected ones become `kind: "candidate"`,
  `selected: false`; keep `uses_capabilities[0]` as the `capability` reference;
  preserve `owner_outcome`, `reason`, `proof_sequence`, `tracking_issue`, and
  `current_limit`; add `state: planned` and empty evidence arrays.

`scripts/capability-map-migration.test.mjs` asserts a version-1 map migrates to
a structurally valid version-2 object and that re-running is a no-op.
`scripts/check.sh` runs the new migration test file and keeps the existing
capability-map test file.

The checked `capabilities/capability-map.json` is authored as version 2 content
directly (the migration exists to prove the v1→v2 transform and to keep the
tooling honest, matching the ticket's "migration from the current JSON schema is
explicit and validated"; the checked file is the migrated result, not a stored
v1 blob).

## Renderer

`renderRoadmapBlock` emits, for the generic capability:

```text
Progressive delegation — product surface missing
  Landed substrate: ...
  Architecture contract: #126 (archived, substrate) — represented, not a blocker
  Execution/review foundations: #127 (archived, substrate), #129
  Scoped evidence/matching: #128
  Proposal-specific evaluation: #133
  Selected proof: recurring Gmail drafts (#130)
  Portability proof: second communication shape (#131)
  Whole-responsibility progression: #132
```

Blockers render as issue links under role headings; they never appear in the
"Repository proof" column, which shows only archived changes, spec paths, and
named tests. The summary count line gains a `proof_in_progress` term when any
capability is in that state. Public-copy rule: if the selected proof is
`shipped` but portability is not `verified`, the renderer says "first shipped
proof" and never claims the capability works across protocols.

## Authority, containment, audit, and failure modes

This change is documentation/tooling and does not touch runtime authority,
containment, or audit. It sharpens the *documentation* truth standard: it
removes a route by which the Gmail proof could be cited as evidence that other
protocols already work. The risk it mitigates is overclaiming, which is a
documentation-integrity concern, not a runtime one. The main failure mode is a
deterministic-renderer mismatch between the JSON and the generated blocks; the
`checkRepository` staleness check and the determinism test cover that, and CI
(`scripts/check.sh`) re-runs the read-only check to confirm the write produced
no drift.

## Trade-offs and rejected alternatives

- **Nested proof objects (option 1):** rejected — couples proof lifecycle to
  capability identity; cannot let #130/#131/#132 advance independently.
- **Stricter categorized wiring rule on all capabilities:** rejected — would
  force relabeling existing vertical tests and churn already-valid records; the
  ticket scopes the stricter rule to the generic capability.
- **New "roadmap" capability spec:** rejected — `openspine-development-process`
  already owns the development-layer truth standard; a parallel spec would
  compete for that ownership.
- **Treating closed issues (#126/#127) as blockers in `blocking_issues`:**
  rejected — they are archived substrate. `blocking_issues` is for open
  blockers only; landed substrate belongs in `runtime_changes`/`canonical_specs`.
