# Tasks

## 1. Re-ground before implementation

- [ ] Confirm #130 (`ship-recurring-gmail-draft-proof`) is archived and `main` is green before starting; this change is HARD-gated on it.
- [ ] Re-read the canonical bodies of every requirement this change modifies and re-derive each `MODIFIED` body as a superset of the current text: `responsibility-contract` ("Reusable delegation MUST validate independent action and implementation declarations", "Reviewed scope matching MUST be protocol-neutral and fail closed"), `gate-action-api` ("Scope-matched admission MUST supply only a kernel-resolved request to the executor"), and `standing-rules` ("Exactly one compatible scoped rule MUST match before any budget moves").
- [ ] Run `npx --no-install openspec validate prove-progressive-delegation-portability --strict` before implementation and after every spec revision.

## 2. Declare the second shape as catalog data

- [ ] Add one `ActionDescriptor` to `delegation_descriptors()` (`crates/openspine-kernel/src/action_catalog_data.rs:19`) for the shared-workspace message shape: `EffectKind::SharedWorkspaceWrite`, `DataDestination::SharedWorkspace`, `reusable_delegation: true`, and `DarkWindowPolicy::Prohibited`. Required scope dimensions MUST include `EffectDestination`, `OutputChannel`, and `BoundParameters` alongside the connector, account, target, counterparty, relationship-tier, workflow, and task-shape dimensions.
- [ ] Add one `ActionImplementationDescriptor` to `implementation_descriptors()` (`:93`) with its own `implementation_id`, `connector_kind`, `executor_id`, `resolver_id`, and explicit versions.
- [ ] Add a literal `egress_declarations()` entry (`:109`). `None`/`None` is a deliberate non-egress classification and MUST be written explicitly; do not rely on a default.
- [ ] Do **not** add the shape to the non-delegable set, the counterparty-facing set (D-057), or the dark-window eligibility allowlist (D-162).

## 3. Kernel resolver, executor, and deterministic connector

- [ ] Add the deterministic in-repo connector modelling workspace, direct-message, and channel visibility semantics. No external credential, OAuth flow, or network dependency.
- [ ] Add the kernel-side resolver that constructs the resolved context for the new shape from kernel resolution only: connector instance, account role/identity, canonical target, counterparty, participant set, and the visibility values. No field may originate in shell-supplied request data.
- [ ] Register the executor in the kernel effect-executor registry so `is_execution_backed` is true for the shape only when both the descriptor and the registered executor are present.
- [ ] Bind visibility through the existing dimensions only — `EffectDestination`, `OutputChannel`, `BoundParameters`. Adding a `ReviewedScopeDimension` variant is out of scope; if a genuine gap appears, record it as an open question in `design.md` and stop.

## 4. Drive the whole path for the new shape

- [ ] Repeated-approval mining produces a candidate for the new shape through the existing typed `DelegationEvidence::repeated_approvals` contract, grouped by context-class digest, with no shape-specific grouping code.
- [ ] The proposal reaches owner review only through the shared `artifact.propose` evaluation core with a passing receipt and an evaluation binding, exactly as the first shape does.
- [ ] Owner approval activates through the ordinary artifact lifecycle with a freshly minted owner activation grant; the proposer's grant is not reused.
- [ ] A later matching request is admitted by exact-one scoped selection, produces exactly one real effect through the shape's registered executor, and returns a responsibility receipt naming the rule id/version, resolved target, and post-reservation quota/rate remaining.
- [ ] Pause, resume, revoke, and expiry act on the bound standing rule through the existing lifecycle controls, with refusals surfacing as typed refusals (D-171).

## 5. Confusion and fallback matrix

- [ ] Cross-shape confusion: a rule reviewed for one shape MUST NOT admit the other, and one shape's executor MUST NOT be reachable with the other's resolved context.
- [ ] Cross-visibility confusion: a direct-message rule MUST NOT admit a channel-visible effect and vice versa; a rule reviewed for one channel MUST NOT admit another; a widened participant set MUST stop the rule matching on `BoundParameters`.
- [ ] Cross-account and cross-target confusion for the new shape, mirroring the first shape's coverage.
- [ ] The full fallback matrix for the new shape, each proven to happen before any effect and to move no budget: erased counterparty, bound-context drift, exhausted quota, exhausted rate, pause, expiry, revocation, unresolved counterparty, evaluation staleness, and a fenced retry.
- [ ] An unregistered executor for the declared shape returns the typed `NoExecutor` outcome and cancels the reservation, restoring full budget — never a successful stub.

## 6. Reachability census and mutation killers

Complete this table before ticking any guard task in section 5. Production callers exclude `*_tests.rs`, `/tests/`, and `_tests`; a blank second or third column is dead or unproven and MUST be reported as such rather than ticked. The test MUST enter at the production caller or higher, not call the guard directly. Record the killing mutation for each row: a test that survives the mutation it claims to kill is not evidence (D-172).

| GUARD SITE | PRODUCTION CALLER | TEST THAT ENTERS AT OR ABOVE THAT CALLER | KILLING MUTATION |
| --- | --- | --- | --- |
| Second-shape descriptor/implementation readiness conjunction | | | |
| Second-shape resolver: every context field is kernel-resolved | | | |
| Visibility dimensions participate in exact-one selection | | | |
| Cross-shape executor rejection | | | |
| Unregistered second-shape executor -> typed `NoExecutor` + cancel | | | |
| Dark-window `Prohibited` enforced for the shared-workspace destination | | | |
| Literal egress declaration present (absent row fails closed) | | | |

- [ ] Fill every column above, or record the row as **UNPROVEN / latent** with the concrete reason it cannot be entered from production. Do not add a direct-wrapper test to turn a blank column green.
- [ ] Record mutation identities and results inline, naming the specific edit made and the specific assertion that died.

## 7. Specs, capability map, verification

- [ ] Keep the `MODIFIED` bodies supersets of the canonical text with no dropped sentence, scenario, or `Test:` anchor; carry pre-seeded requirements as `## MODIFIED Requirements`, never re-`ADDED`.
- [ ] Every `Test:` anchor and every census test name MUST resolve to a real test; verify by grep before committing, since these land in canon on archive.
- [ ] Add the conformance tests for the capability map's `second-protocol-portability` proof. That proof may only move to `verified` with non-empty `conformance_tests` (`scripts/capability-map.mjs:413-421`); populating `owner_path_tests` alone is not sufficient.
- [ ] After archive, regenerate the capability map with `node scripts/capability-map.mjs --write` and never hand-edit inside the roadmap markers. `progressive-delegation`'s `current_limit` must stop saying portability is unverified once the proof carries evidence.
- [ ] Run `./scripts/check.sh prove-progressive-delegation-portability` and the named owner-path tests by exact name.
- [ ] Do not generalize connector or task-shape abstractions; #132 consumes this change as design evidence and owns that work.
