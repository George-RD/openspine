# Tasks

## 1. Author the version 2 schema, migration, and corrected records

- [x] Bump `capabilities/capability-map.json` to `schema_version: 2` and extend
      each `capabilities[]` entry with `generic` (boolean) and `blocking_issues`
      (`[{ issue: integer, role: string }]`), where `role` is one of
      `architecture contract`, `execution/review foundations`, `scoped
      evidence/matching`, `proposal-specific evaluation`, or a future role.
- [x] Add a new capability `state` value `proof_in_progress` alongside
      `runtime_landed`, `product_surface_missing`, and `wired_into_lyra`.
- [x] Allow each `owner_path_tests` entry to carry a `kind` of
      `positive_effect`, `fallback`, or `control`.
- [x] Replace `starter_workflow_candidates[]` with a top-level `proofs[]` array
      of `{ id, capability, kind, selected, owner_outcome, reason, scope,
      current_limit, state, tracking_issue, proof_sequence?, owner_path_tests,
      conformance_tests }` where `kind` is `selected`, `portability`,
      `whole_responsibility`, or `candidate` and `state` is `planned`,
      `in_progress`, `shipped`, or `verified`.
- [x] Create `scripts/capability-map-migration.mjs` exporting
      `migrateCapabilityMap(map)`: idempotent (returns unchanged when
      `schema_version === 2`); for version 1, set `schema_version: 2`, add
      `generic: false` and `blocking_issues: []` to every capability, rename
      `starter_workflow_candidates` to `proofs`, convert the previously selected
      candidate to `kind: "selected"` moving `task_boundary` into `scope`,
      convert unselected candidates to `kind: "candidate"`, preserve
      `uses_capabilities[0]` as `capability`, and add `state: planned` with
      empty evidence arrays.
- [x] Create `scripts/capability-map-migration.test.mjs` asserting a version 1
      map migrates to a structurally valid version 2 object and that re-running
      is a no-op.
- [x] Update `scripts/check.sh` to run
      `node --test scripts/capability-map-migration.test.mjs` next to the
      existing capability-map test invocation.
- [x] Rewrite `capabilities/capability-map.json`: replace
      `recurring-draft-responsibility` with a generic capability
      `progressive-delegation` (`generic: true`, `state: product_surface_missing`,
      protocol-neutral `owner_outcome`); populate `runtime_changes` and
      `canonical_specs` with the confirmed archived substrate (standing rules,
      reflection miner, governed artifact lifecycle, audit artifact store,
      budget/spend kill switch, grant chain); populate `blocking_issues` with
      #128 (`scoped evidence/matching`), #129 (`execution/review foundations`),
      #133 (`proposal-specific evaluation`) — #126/#127 are archived substrate,
      not blockers.
- [x] Keep `selected-gmail-draft` as a shipped vertical capability distinct from
      the generic capability.
- [x] Add proofs referencing `progressive-delegation`: `recurring-gmail-draft`
      (`kind: selected`, `selected: true`, `tracking_issue: 130`, five-step
      `proof_sequence`), `second-protocol-portability` (`kind: portability`,
      `tracking_issue: 131`), `whole-responsibility-progression`
      (`kind: whole_responsibility`, `tracking_issue: 132`), with empty evidence
      arrays (not yet shipped).
- [x] Keep `task-and-reminder`, `research-and-brief`, `skill-install-proof` as
      `kind: candidate` proofs with `selected: false`.

## 2. Extend the validator with the new truth rules (TDD)

- [x] Update the `validMap()` factory and `fixtureRoot()` helper in
      `scripts/capability-map.test.mjs` to produce a valid version 2 map (a
      generic capability with categorized owner-path tests, a top-level
      `proofs[]` array, and fixtures for referenced test/spec/artifact paths).
- [x] Add tests asserting: the generic capability and a proof are distinct
      objects; exactly one proof has `selected: true`; a proof with
      `state: shipped` and empty `owner_path_tests` fails; a generic capability
      marked `wired_into_lyra` with only `runtime_changes` (no positive-effect /
      fallback/control owner-path tests) fails; a portability proof marked
      `state: verified` with empty `conformance_tests` fails; a
      whole-responsibility proof marked `verified` while the selected proof is
      not shipped or portability is not verified fails.
- [x] Add a test asserting `blocking_issues`/issue numbers are rejected wherever
      runtime evidence is expected (e.g. an integer in `runtime_changes` fails).
- [x] Enforce in `validateCapabilityMap`: `schema_version === 2`; generic
      wired capability requires at least one `positive_effect` owner-path test
      and at least one `fallback` or `control` owner-path test; a shipped proof
      requires non-empty `owner_path_tests`; a `kind: portability` proof with
      `state: verified` requires non-empty `conformance_tests`; a
      `kind: whole_responsibility` proof cannot be `verified` unless the
      selected proof is `shipped` and the portability proof is `verified`.
- [x] Keep the existing archived-change, path-existence, and registered-test
      checks intact against the new fixtures, applied to both
      `owner_path_tests` and `conformance_tests`.

## 3. Update the renderer and regenerate the roadmap

- [x] Update `renderRoadmapBlock` in `scripts/capability-map.mjs`: extend the
      summary count line to include `proof_in_progress`; render the generic
      capability with its landed substrate, role-grouped `blocking_issues`
      (labeled `Architecture contract`, `Execution/review foundations`, `Scoped
      evidence/matching`, `Proposal-specific evaluation`) as issue links, and
      `Selected proof` / `Portability proof` / `Whole-responsibility
      progression` lines each linked to its tracking issue; keep
      `evidenceMarkdown` showing only archived changes, specs, and named tests.
- [x] Apply the public-copy rule: when the selected proof is `shipped` but
      portability is not `verified`, render "first shipped proof" and do not
      claim cross-protocol operation.
- [x] Add a rendering determinism test: blockers grouped by role, the three
      proof lines render with their issue links, and issue numbers do not
      appear in the proof column.
- [x] Keep `checkRepository` generation confined to
      `site/src/content/docs/roadmap.md`; never write into
      `openspec/openspine-change-sequence.md`.
- [x] Run `node scripts/capability-map.mjs --write` to regenerate the roadmap
      block; confirm the read-only invocation passes its staleness check.

## 4. Verify

- [x] `node_modules/.bin/openspec validate
      separate-progressive-delegation-roadmap-truth --strict` passes.
- [x] `node --test scripts/capability-map.test.mjs` passes.
- [x] `node --test scripts/capability-map-migration.test.mjs` passes.
- [x] `node scripts/capability-map.mjs` reports source, evidence, and public
      roadmap are consistent.
- [x] `./scripts/check.sh` is green.
