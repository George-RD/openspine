# Design: Make reusable-authority evaluation proposal-specific and evidence-backed

This document describes what shipped. Where the original design named an API or a
mechanism that turned out not to exist or not to fit, the section says so and
describes what replaced it, so the archived record is not a plan mistaken for an
account.

## Keep the promotion boundary, replace the evidence

The AD-142 structural guarantee is correct and untouched. `ReplayPassed` and
`JudgePassed` (`overlay_eval_gate/mod.rs`) keep private fields and no public
constructor; `Store::promote_authority_bearing_proposal` remains the only path
that can move `validated -> review_required`, still consumes both witnesses by
value, and still re-derives the digest from the stored row.

What changed is what a witness costs. Before, one was minted for "the owner has
spoken at least once" and "the action id is in the catalog". Now it requires a
complete kernel-assembled input, a structural pass, and — for a scope-bound rule
— a two-sided executed-case ledger. The boundary's type signature is unchanged,
so no call site can drift around it while the evaluator grows.

## The evaluation input is assembled, never accepted

`CanonicalEvaluationInput` has private fields and no public constructor and is
built only by `eval_input::assemble` from kernel sources. The proposal
contributes its own declared content and nothing else: it cannot assert its own
executor readiness, its own policy standing, or which rules it overlaps.

| Input dimension | Source | Failure |
| --- | --- | --- |
| Catalog non-delegability | `ActionCatalog::is_non_delegable` | judge denial |
| Composed deny set | policies in the active registry | judge denial |
| Executor readiness | effect-executor registry **or** action-handler registry | judge denial (scope-bound only) |
| Delegation + implementation descriptor | `delegation_descriptor_for`, `implementation_descriptor_for_action` | `NoImplementationDescriptor` |
| Reviewed scope binding | the proposal's `ReviewedScopeBinding`, checked against `required_scope_dimensions` | `MissingDimension`, `InconsistentScopeBinding`, `ScopeMissingRequiredDimensions` |
| Active rules for the action | read-only `active_standing_rules_for_action` | judge denial |
| Epoch set | see below | recorded, never fabricated |

The original design named `ActionCatalog::validated_delegation_contract` and an
`IncompleteInput { dimension }` struct variant. The shipped code calls
`validate_delegation_contract` on the two descriptors directly, and
`IncompleteInput` is a set of tuple variants each naming its dimension. Same
guarantee, different shape.

**Readiness is a scope-bound axis.** It asks whether the *named* implementation
has a registered executor, so it applies only where a reusable-delegation
descriptor names one. An earlier revision applied it to every standing rule and
refused `connector.enable`, which the runtime handles through a different path;
that was over-reach and was corrected. Readiness is satisfied by either real
execution path — a registered effect executor or a registered action handler.

## Every standing rule gets the authority axes

The decisive structural point, and the one review caught: a standing rule admits
its action *without per-instance owner approval* whatever shape that action has
in the catalog. Routing rules for actions without a reusable-delegation
descriptor to the weaker catalog-membership arm left a non-delegable,
policy-denied rule unexamined — and exactly one action in the catalog carries
such a descriptor, so that was nearly every rule.

So the arms split by *what can be checked*, not by *whether to check*:

- **Always:** manifest invariants, catalog membership, catalog non-delegability,
  composed policy deny.
- **Where no reusable-delegation descriptor exists:** the action must be declared
  *approval-narrowing*, because such a rule grants blanket authority.
- **Where a reusable-delegation descriptor exists:** executor readiness, contract
  eligibility, dark-window admissibility, budget and expiry bounds, overlap and
  widening, and executed-case replay.

## Blanket authority needs its own licence

Two holes only became visible once every standing rule took the
reusable-authority arm.

**Catalog membership.** Routing all standing rules to that arm made
`catalog_structural_arm`'s `UnknownAction` raise unreachable — and every
remaining axis reads catalog metadata, so an id nobody declared is absent from
all of it and answers "no" everywhere. A rule for `totally.not.a.real.action`
passed. `contains` is now an authority axis.

**Effectful actions.** Using "has no reviewed scope" as the licence to skip the
scope-bound axes was the same hole one layer down, and it landed on exactly the
actions that matter: `email.send`, `secret.rotate`, `filesystem.host_write`,
`network.raw_egress`, `coolify.deploy` and `policy.modify_direct` are all
catalogued, all effectful, none non-delegable, and none carries a delegation
descriptor.

The catalog **cannot** express "effectful" today, and this was checked rather
than assumed: `egress_declarations` is `None`/`None` for both `email.send` and
`connector.enable`; `counterparty_facing_actions` holds only `email.send`;
`non_effect_stub_actions` is seven READ ids that exclude `connector.enable`; and
`EffectKind` lives on `ActionDescriptor`, which by definition these actions lack.

So the smallest addition that expresses the distinction is an explicit,
fail-closed allowlist: `ActionCatalog::approval_narrowing_actions`. An action in
it may carry a standing rule binding no reviewed scope, because such a rule
narrows an *approval requirement* rather than admitting an effect. Everything
else must bind a reviewed scope. It holds two entries, each justified at the
registration site: `connector.enable` (no dispatchable executor exists at all)
and `openspine.status.read` (a kernel-status read the reflection miner mines
into a rule). Entries are added on demonstrated need, never speculatively —
the set is the licence to grant blanket reusable authority, so it must stay
small enough to read in one sitting.

## Replay without a corpus: derive cases from what the proposal already binds

There is no store of prior `ResolvedActionContext` rows — the type appears in
`store/` only as a function parameter, never as a column — so the case set is
*derived*, not *retrieved*:

- the **baseline** case reconstructs the resolved context the reviewed scope
  describes and must match;
- **changed-context** cases perturb exactly one bound instance dimension and must
  be refused, with the mismatch attributed to that dimension.

Each case builds a real `ResolvedActionContext` through `try_new` and decides it
with `ReviewedActionScope::compare`. That matters for honesty: `try_new`
re-derives the declaration axes from the live catalog, so the baseline genuinely
fails under catalog drift rather than trivially matching itself.

One dimension per case is what makes a failure attributable. Declaration axes are
listed explicitly rather than caught by `_`, so a newly added instance dimension
fails to compile here instead of silently going uncovered — `AccountRole` and
`RelationshipTier` were swallowed by a catch-all until review caught it.

The original design also promised positive cases reconstructed from a sealed
`DelegationEvidence` set, and budget/policy/executor/overlap/dark-window case
variants. Neither shipped. `DelegationEvidence` exists only in schemas' test
files, so there is no production type to read; and the other variants are pure
properties of the proposal rather than of a varied context, so each is a judge
axis that refuses outright instead of a simulated case reporting a second opinion
on a settled question.

## The ledger is the anti-degradation mechanism

The verdict stores one entry per case: kind (`reviewed_scope_baseline` or
`dimension_mutation`), the mutated dimension, expected and observed outcome, and
whether the mismatch was attributed to the varied dimension.

Replay denies unless the ledger is non-empty, at least one case matched, at least
one changed-context case was refused, and every case did what it was constructed
to do. A corpus count produces no ledger, so "replay" cannot silently become a
presence check again. There is deliberately no fitness number for this evaluator:
a score is exactly what let `1.0` be recorded for counting conversation turns.

A standing rule whose action has no reusable-delegation descriptor has no scope
to vary. Its replay falls to the owner-history availability check, which is
reported under the name `owner-control-history-availability` and never as a
replay.

## Judge before replay

Both must pass, so order does not change what is admitted — but it changes which
denial the owner sees. Running the structural judge first surfaces "no registered
executor" instead of masking it behind "no owner history".

## Epoch binding and read-time staleness

| Epoch | Source | Detects |
| --- | --- | --- |
| `proposal_digest` | stored proposal bytes | the proposal itself changed |
| `compatibility_digest` | the binding's compatibility epoch | descriptor, implementation, executor, resolver, egress or required-dimension change |
| `reviewed_scope_digest` | the binding's scope key | the reviewed instance scope changed |
| `evidence_set_digest` | not recorded today | — |
| `descriptor_version`, `implementation_version`, `policy_version` | catalog and registry | declared version movement |

Currency is computed at read time from stored columns against live values.
Nothing sweeps and nothing rewrites a row, so there is no window in which a stale
verdict reads fresh and no crash can strand a half-finished pass.

**Activation re-checks; promotion cannot.** Promotion computes the epochs it
stores in the same operation, so it can never observe a stale verdict — the axis
it enforces is the digest binding both witnesses already carry. Activation
compares before its transaction opens, so the refusal audit survives. The
`proposal_digest` axis is projected out of the activation comparison because the
YAML bytes live in the artifact store and cannot be re-derived there.

Two known limits, recorded rather than papered over: `compatibility_digest` and
`reviewed_scope_digest` are read back from the same manifest being activated, so
those two axes detect catalog movement only through the versions beside them; and
`disclosure.rs` activates a standing rule directly, bypassing this check. D-164's
text is qualified accordingly.

## Owner copy is rendered, not written

The summary is a pure function of the two verdicts' recorded evidence. It reports
executed-case counts and passing axes, and names each evaluator from its own
evidence rather than a hardcoded label — an earlier revision hardcoded
`(replay: pass)` and so printed the word on a proposal where nothing ran, which
its own test caught.

The propose-time gate summary is a different surface from the owner review object
that #129 renders. This change owns the former only.

## Dark-window cases against the substrate that exists

`bound-dark-window-exceptions` (#135) has no artifacts or symbols in any tree, so
the judge asserts only rules already on `main`: `Prohibited` forbids any
configuration; `DenyOnly` bounds the timeout and forbids an `Allow` default; and
`validate_delegation_contract` already refuses `BoundedAllow` on a communication
or connector-write effect (D-146). When #135 lands its exception vocabulary the
case set extends; nothing here guesses at its names.

## Authority, containment, audit, failure modes

**Authority.** Evaluation mints nothing. It produces two witnesses whose only
power is to permit `validated -> review_required`. Activation still requires
digest-bound owner approval and #128's activation-time scope-binding guard.

**Containment.** Replay executes no connector effect and touches no external
system. The input is kernel-assembled; the shell and the miner cannot supply a
dimension. Case generation mutates only in-memory copies.

**Audit.** A stale-verdict refusal appends `eval_verdict.stale_at_activation`
naming the stale axes. Verdict rows remain append-only.

**Failure modes.** Unresolvable input → typed `IncompleteInput` naming the
dimension. Structural axis failure → typed denial naming the axis. Empty or
one-sided ledger → replay denial. Stale epoch at activation → refusal. All keep
the proposal outside the approval surface (D-004).

**Prompt injection.** Proposal content is data, read only through typed schema
accessors. Every trusted value — descriptors, readiness, deny set, epochs — is
read from the kernel.

## Rejected alternatives

| Option | Why rejected |
| --- | --- |
| Keep the owner-history availability check and rename it honestly | Honest naming alone still lets an unexecutable, policy-denied or ambiguously overlapping proposal reach the owner. The done-when requires refusal, not relabelling. |
| Build a durable corpus of prior `ResolvedActionContext` rows and replay against it | No such store exists; creating one is a schema and retention question of its own, and a thin version would re-introduce "corpus present, therefore pass". |
| Score the evaluation and promote above a threshold | A threshold turns a missing case into a slightly lower number. `fitness: Some(1.0)` for counting turns is exactly this failure. |
| Let replay call a private matching predicate tuned for evaluation | Two matchers drift, and the evaluation one would be the untested copy. Replay calls `try_new` + `compare` so a matching bug fails both surfaces at once. |
| Route rules for descriptor-less actions to the catalog-membership arm | This shipped in an earlier revision and was the change's blocker: exactly one action carries a delegation descriptor, so nearly every standing rule escaped the authority axes entirely. |
| Apply executor readiness to every standing rule | Over-reach in the opposite direction: it refused `connector.enable`, which has no named implementation to be ready. Readiness is a scope-bound axis. |
| Use "no reviewed scope" as the licence to skip the scope-bound axes | The same hole one layer down. Every effectful action without a delegation descriptor — `email.send` among them — would carry blanket unscoped authority. Blanket authority needs its own explicit licence. |
| Derive "effectful" from existing catalog metadata | Checked and rejected on evidence: egress declarations are `None`/`None` for both the action that must pass and the one that must fail, counterparty-facing holds only `email.send`, and `EffectKind` lives on the descriptor these actions lack. |
| Mark verdicts stale with a background sweeper | Adds a window where a stale verdict still reads fresh, and a crash strands the sweep. Read-time comparison has neither. |
| Mutate several scope dimensions per adversarial case | Proves only that *something* changed. One dimension at a time makes the ledger attributable and catches a dimension that is silently not compared. |
| Run budget/policy/executor/overlap checks as replay cases too | Each is a property of the proposal, not of a varied context. Simulating them would report a second opinion on a question the judge already answered. |
| Consume `bound-dark-window-exceptions` symbol names ahead of that change | Those symbols exist in no tree. Guessing would produce a delta that compiles against neither the current nor the future shape. |
