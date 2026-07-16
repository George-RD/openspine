# Proposal: Harden gate trusted paths

## Summary

Enumerate, classify, and test every effectful path that reaches around `gate()`,
and move two validations that today live in dispatch into the pure `gate()`
decision so the kernel — not the shell — is the authority on token possession
and digest integrity (AD-120: the shell sends intents, the kernel computes
outcomes). Concretely: (1) the trusted-path carve-outs become enumerated catalog
data with one test per entry; (2) kernel-originated effects gain a `KernelOrigin`
marker and route through `gate()` (exempt from approval, never from audit); (3)
selection-token validation moves into pure `gate()` while single-use consumption
stays at dispatch; (4) the kernel re-derives payload/target digests from
artifact-store bytes at approval-effect time and never trusts a shell-supplied
digest.

## What Changes

- `ActionCatalog` gains an enumerated kernel-origin action set and a
  token-requiring flag. Every effect path around `gate()` is classified in the
  spec (`gated-shell` / `post-gate-approved-effect` / `kernel-origin-gated` /
  `internal-maintenance-non-effect`), and each of the eight enumerated entries
  gets a dedicated characterization test.
- A new `ActionOrigin::{Shell, Kernel}` marker distinguishes shell-initiated
  intents from kernel-initiated effects. `notify_owner_best_effort` (the D-046
  trusted courtesy-notice path) routes through `gate()` with `Kernel` origin:
  exempt from approval, never from audit. Kernel-origin calls for actions outside
  the enumerated trusted-origin set are denied.
- Selection-token validation (bound-to-grant, exists, type, not-expired) moves
  into pure `gate()` via `GateContext::find_selection_token` for catalog-marked
  token-requiring actions. The atomic single-use CONSUME stays at dispatch so
  `gate()` remains free of state mutation.
- Shell-facing request DTOs structurally cannot carry digest fields. At
  approval-effect time the kernel re-derives the payload digest from
  artifact-store bytes (target digest re-derivation already exists at
  `crates/openspine-kernel/src/pipeline/approval.rs:290`) and denies on mismatch.
  `gate()` no longer relies on caller-supplied digest strings.

## Why

Canon kernel-readiness item 4 + AD-120. The gate scout found that `gate()`
(`crates/openspine-gate/src/gate.rs:67`) trusts `req.payload_ref.digest` and
`req.target_digest` verbatim (gate.rs:142–150) and that `GateContext::
find_selection_token` is declared (gate.rs:35) but never called — selection-token
validation lives only in dispatch (`crates/openspine-kernel/src/api/actions.rs:
384–421`), consumed post-gate. Today integrity holds only because approval-callback
requests are re-read from the store, not because `gate()` re-hashes bytes or checks
token possession. That is a contract gap, not a guarantee: the gate's deny-by-
default and digest-bound invariants (D-004, D-011) must be enforced where the
decision is made, not delegated to dispatch.

## Affected layer

OpenSpine core — the `openspine-gate` crate (`gate()` purity + `ActionOrigin` /
catalog metadata) and the kernel effect layer (`pipeline`, `api`, approval
handlers). This is authority-sensitive: it changes gate semantics, approval
requirements, audit behavior, and the kernel/shell trust boundary (D-005/D-010).

## Authority sensitivity

**HIGH — gate semantics.** This change rewrites how `gate()` decides: it adds a
kernel-origin approval exemption, moves token validation into the decision, and
replaces digest trust with kernel re-derivation. Deny-by-default (D-004), grant-
is-only-live-authority (D-007), and digest-bound approval (D-011) are directly
affected. The change is strictly narrowing: every new path is still mediated by
`gate()`, kernel-origin effects are audited, and shell-supplied digests are
rejected outright.

## Goals

- Every effectful path around `gate()` is enumerated, classified, and covered by a
  dedicated test — no silent carve-out.
- Kernel-origin effects are exempt from *approval* but never from *audit*, and are
  routed through `gate()` so the audit event is always emitted (generalizing the
  single D-046 `owner.notified` carve-out into a data-described trusted-origin
  set).
- Selection-token validation is a property of the `gate()` decision, not of
  dispatch, while `gate()` stays pure (no mutation).
- Digest integrity is computed by the kernel from artifact-store bytes, never
  trusted from the shell.

## Non-goals

- No relaxation of deny-by-default or of the shell→kernel trust boundary.
- No new external effects; the eight enumerated paths are exactly the paths that
  exist today.
- No change to the driver module's non-invocation of `gate()`
  (`pipeline-driver` req "The driver MUST NOT invoke gate()") — `notify_owner_
  best_effort` routes through `gate()` at the effect layer (`pipeline/mod.rs`),
  not inside the driver prefix.
- No threat-claims register edits and no decision-log edits in this change
  directory (both are listed as implementation tasks in `tasks.md`).

## Dependencies

None. The change stands alone; it requires no other in-flight change.

## Problem/Context

`gate()` is the single mediation point every effectful action must pass through,
but its surrounding carve-outs are implicit and scattered: one documented
trusted path (D-046 `owner.notified`), token validation buried in dispatch, and
digest fields trusted verbatim. The kernel-readiness item 4 + AD-120 mandate that
the carve-out set be explicit and that token possession and digest integrity be
kernel-computed, not shell-asserted.

## Proposed Solution

The four settled design decisions (D-055.1–D-055.4) in `design.md`: enumerate
carve-outs as catalog data; add `KernelOrigin`; move token validation into pure
`gate()` (consume stays at dispatch); re-derive digests kernel-side.

## Acceptance Criteria

- `openspec validate harden-gate-trusted-paths --strict` is green.
- Each of the eight enumerated effect paths has a characterization test asserting
  its gate-decision and audit-event behavior.
- `gate()` denies a missing/expired/foreign/wrong-type selection token for a
  token-requiring action, and denies a kernel-origin call outside the enumerated
  trusted-origin set.
- Approval-effect handlers deny when the kernel-re-derived payload/target digest
  mismatches the approved digest.

## Out of Scope

Grant-chain/mode semantics (`define-grant-chain-and-modes`), identity store
(`implement-identity-store-and-principal`), and workflow state machines are
separate changes. This change only hardens the existing `gate()` boundary and its
immediate carve-outs.

## Decision-log check

This change settles four new decisions as **D-055** (carve-out enumeration as
catalog data; `KernelOrigin` marker with approval-exempt/audit-never-exempt
routing; selection-token validation inside pure `gate()` with dispatch-side
consumption; kernel-re-derived digests at approval-effect time). D-055 is added to
`.raw/openspine-decision-log.md` as an implementation task in `tasks.md` (this
change directory edits no decision-log file).

D-055 is consistent with, and refines, three accepted precedents:

- **D-041** — `email.create_draft` digest composition (`payload = {subject, body}`,
  `target = {thread_id, connector, account_role, recipients}`). Decision D-055.4
  makes the kernel *re-derive* those very digests from artifact-store bytes
  instead of trusting a supplied string, closing the gap D-041 left open.
- **D-046** — grant budgets (and the `owner.notified` trusted courtesy-notice
  carve-out) are enforced kernel-dispatch-side; kernel-originated owner
  notifications are trusted-but-audited, not gate-mediated. D-055.2 generalizes
  that single carve-out into an enumerated `KernelOrigin` set that *does* route
  through `gate()` (exempt from approval, never from audit) — strengthening, not
  reversing, D-046.
- **D-050** — `max_model_calls` is enforced with an atomic upsert, not a
  count-then-compare. D-055 reaffirms kernel-side enforcement placement and
  extends the "kernel computes, shell asserts nothing" principle to token
  validation and digest re-derivation.

No accepted decision is reversed or weakened. If implementation reveals a need to
refine an accepted decision, the decision log is updated before the change
completes.
