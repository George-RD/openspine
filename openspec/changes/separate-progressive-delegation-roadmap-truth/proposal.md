# Separate generic capability, selected proof, and portability evidence in the checked roadmap

## Dependencies

- `define-responsibility-contract` (archived, **HARD**): supplies the D-146
  protocol-neutral two-axis responsibility contract and the reviewed-scope
  vocabulary this change's schema draws on. The capability map must name a
  protocol-neutral generic capability distinct from any vertical proof, which
  is only meaningful once D-146 established that responsibilities are
  protocol-neutral reference views over ordinary workflow/standing-rule
  authority.
- This change is a truthful roadmap/tooling slice. It may land before the
  runtime foundations #128/#129/#133 because it only *represents* those open
  blockers as blockers; it never marks any missing capability complete. It is
  independent of `unify-approved-and-delegated-effect-execution` (#127,
  archived) for its data model, though the current records reference #127 as
  landed substrate.
- Canonical decisions: D-146.

This change affects **development tooling** and the **checked documentation
truth standard**. It does not affect OpenSpine core runtime authority, private
data, external communication, connector access, or system operations. It
changes no runtime behavior: it only changes how the capability map and its
validator/renderer represent capability, proof, and portability truths.

## Problem/Context

The current checked capability map names the missing capability as recurring
Gmail drafts (`recurring-draft-responsibility`) and then also selects recurring
Gmail drafts (`recurring-gmail-draft-responsibility`) as the starter proof.
That conflates three different truths:

1. **generic capability** — protocol-neutral progressive delegation / reviewed
   reusable responsibility;
2. **selected vertical proof** — recurring Gmail draft creation for one reviewed
   scope;
3. **portability status** — whether a second materially different protocol has
   demonstrated that the architecture is not secretly Gmail-specific.

This confusion propagates into the generated public roadmap and makes
implementation details look like the product architecture. The capability map
exists to prevent documentation overclaiming, so its schema must be able to
represent the corrected dependency/evidence model.

A second, sharper problem is evidence discipline. The map currently keys
capability records only by `runtime_changes` (archived change ids),
`canonical_specs`, `lyra_artifacts`, and `owner_path_tests`. It has no way to
distinguish "this is a blocker, tracked by an issue" from "this is landed,
proved by an artifact or test." The issue #134 body and the reviewer expectation
are that **issue numbers are blockers, never runtime evidence**; open issues
must never render as shipped features. The schema must separate the two.

A third problem is the wiring standard. A capability can be marked
`wired_into_lyra` with only a runtime change and one named owner-path test
today. For a *generic* capability that claim must be stricter: it cannot be
marked wired without named owner-path tests for a real positive effect **and**
the required fallback/control cases, because a proposal renderer, a
standing-rule activation test, or a runtime primitive alone cannot prove the
owner-facing generic path works.

## Proposed Solution

1. **Bump the capability-map schema to version 2**, separating generic
   capability, selected proof, portability evidence, and whole-responsibility
   maturity. Capability records gain `generic` (boolean) and `blocking_issues`
   (`{issue, role}`). `starter_workflow_candidates[]` is replaced by a
   first-class top-level `proofs[]` array whose entries reference a capability
   id and carry a `kind` (`selected`, `portability`, `whole_responsibility`,
   `candidate`), a `state` (`planned`, `in_progress`, `shipped`, `verified`),
   `tracking_issue`, and evidence fields (`owner_path_tests` for the selected
   proof, `conformance_tests` for the portability proof). Add a capability
   `state` value `proof_in_progress`.

2. **Add an explicit, idempotent, tested migration** from version 1 to version
   2 (`scripts/capability-map-migration.mjs`), following the repo
   `<subject>_migration` naming convention, so the current checked JSON can be
   reproduced deterministically and re-running is a no-op.

3. **Correct the current records.** `recurring-draft-responsibility` becomes a
   generic capability `progressive-delegation` (`generic: true`, `state:
   product_surface_missing`) with its landed substrate (standing rules,
   reflection miner, governed artifact lifecycle, task grants and grant chain,
   budgets, expiry, revocation, and audit) listed in `runtime_changes` /
   `canonical_specs` and its open blockers listed in `blocking_issues`.
   `selected-gmail-draft` stays a shipped vertical capability, distinct from
   the generic one. Three proofs reference `progressive-delegation`:
   `recurring-gmail-draft` (selected, #130), `second-protocol-portability`
   (portability, #131), and `whole-responsibility-progression`
   (whole_responsibility, #132). The other pre-existing candidates remain as
   `candidate` proofs.

4. **Encode the CI truth rules in the validator**, driven by tests
   (TDD). Key rules:
   - a generic capability cannot be `wired_into_lyra` without named owner-path
     tests for a real positive effect and the required fallback/control cases;
   - a shipped proof requires non-empty `owner_path_tests`;
   - portability cannot be `verified` without non-empty `conformance_tests`;
   - whole-responsibility cannot be `verified` unless the selected proof is
     `shipped` and the portability proof is `verified`;
   - issue numbers (`blocking_issues`, `tracking_issue`) are never runtime
     evidence — they cannot appear where evidence is expected;
   - every evidence path and test name remains checked, as today.

5. **Regenerate the deterministic renderer output** into the generated block of
   `site/src/content/docs/roadmap.md` (between
   `<!-- capability-map:start -->` / `<!-- capability-map:end -->`), never
   hand-editing inside the markers. The roadmap shows the generic capability,
   its landed substrate, role-grouped blockers, and the selected /
   portability / whole-responsibility maturity lines, without rendering open
   issues as shipped features. The capability map never writes into
   `openspec/openspine-change-sequence.md`, whose file header states it holds
   only the change decomposition.

## Acceptance Criteria

- The checked source and public roadmap call progressive delegation the
  capability and Gmail the first proof.
- CI can distinguish runtime substrate, selected proof, wired owner outcome,
  portability, and whole-responsibility maturity.
- CI rejects a wired generic capability without real owner-path tests.
- CI rejects portability without second-protocol conformance evidence.
- Issue numbers render as blockers, never as repository proof.
- The generated roadmap remains deterministic and replaces only its generated
  block.
- Migration from the current JSON schema (version 1) to version 2 is explicit,
  validated, and idempotent.
- #130/#131/#132 can advance their own evidence fields without rewriting the
  generic capability identity.
- `node_modules/.bin/openspec validate
  separate-progressive-delegation-roadmap-truth --strict` passes, and every
  delta requirement whose header already exists in its pre-seeded capability
  spec is carried as `## MODIFIED Requirements`.

## Out of Scope

- Implementing any runtime foundation. #128/#129/#133 remain open blockers and
  this change does not advance, merge, or mark them complete.
- Re-authoring the Gmail proof. The existing `selected-gmail-draft` capability
  and its proof are preserved as-is, only relabeled as the first vertical proof
  rather than the capability itself.
- Changing how proofs actually run. `proof_sequence` steps are descriptive
  documentation of the intended progression, not runtime contracts.
- Deciding the `ResponsibilityManifest` durable form; that remains #132's
  explicitly deferred architecture decision, and this change only reserves a
  `whole_responsibility` maturity stage for it.
