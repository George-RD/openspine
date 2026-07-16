# Design: Harden gate trusted paths

## Approach

Enumerate, classify, and test every effectful path that reaches around `gate()`,
then move two validations that today live only in dispatch into the pure `gate()`
decision — so the kernel, not the shell, is the authority on token possession and
digest integrity (AD-120: the shell sends intents, the kernel computes outcomes).
Four settled decisions (D-055.1–D-055.4) are encoded below with file:line evidence
from the gate scout report. The work is strictly narrowing: no effect path escapes
`gate()`, kernel-origin effects are always audited, and shell-supplied digests are
rejected outright.

### 1. Carve-out enumeration as data (D-055.1)

The trusted-path carve-outs around `gate()` are today implicit and scattered. They
become enumerated catalog data: `ActionCatalog` gains an enumerated kernel-origin
action set and a per-action `token_requiring` flag. Each of the eight enumerated
effect paths (see table below) is classified as one of `gated-shell`,
`post-gate-approved-effect`, `kernel-origin-gated`, or
`internal-maintenance-non-effect`, and each classified entry gets one dedicated
characterization test asserting its gate-decision and audit-event behavior
(`crates/openspine-gate/src/gate/tests.rs` for pure-decision tests;
`crates/openspine-kernel/src/api/*_tests.rs` and
`crates/openspine-kernel/src/pipeline/tests/approval.rs` for integration).

`gate()` itself (`crates/openspine-gate/src/gate.rs:67`) stays a pure decision
function returning `GateOutcome { decision, audit }` with no I/O
(gate.rs:1–6, 53–60); the enumeration is data the catalog carries, not logic
`gate()` special-cases.

### 2. KernelOrigin marker (D-055.2)

A new `ActionOrigin::{Shell, Kernel}` marker distinguishes shell-initiated intents
from kernel-initiated effects. `notify_owner_best_effort`
(`crates/openspine-kernel/src/pipeline/mod.rs:147–157`, audited as `owner.notified`
at mod.rs:150, trusted-and-ungated by D-046 at mod.rs:141–144) is re-expressed as
a catalog-registered `owner.notify` action that routes through `gate()` with
`ActionOrigin::Kernel`. The decision logic: a kernel-origin call for an action in
the enumerated trusted-origin set is auto-allowed (exempt from approval) but `gate()`
MUST still emit `AuditMeta` (never exempt from audit); a kernel-origin call for an
action outside the set is denied. This generalizes the single D-046 carve-out into
a data-described trusted-origin set without adding ceremony to the kernel
gating itself against itself.

### 3. Selection-token validation into gate() (D-055.3)

`GateContext::find_selection_token` is already declared in the trait
(`crates/openspine-gate/src/gate.rs:35`) and implemented by `Store`
(`crates/openspine-kernel/src/api/mod.rs:65–76`) but is **never called** in
`gate()`'s body today — token validation lives only in dispatch
(`crates/openspine-kernel/src/api/actions.rs:384–421`: parse, bound-to-grant :388,
exists :394, type `EmailThreadSelection` :400, expiry :405, atomic consume :413).

Decision: for catalog-marked `token_requiring` actions, pure `gate()` validates the
token via `find_selection_token` — bound-to-grant, exists, correct type, not
expired — and denies if any check fails. The atomic single-use **CONSUME** stays at
dispatch (after `gate()` returns allow) so `gate()` never mutates state. This is
the explicit purity split: **`gate()` validates, dispatch consumes.**

### 4. Digest re-derivation (D-055.4)

`gate()` currently trusts `req.payload_ref.digest` (gate.rs:142) and
`req.target_digest` (gate.rs:143) verbatim — it never re-derives from bytes or
grant. Today that holds only because the approval-callback request is re-fetched
from the store (`crates/openspine-kernel/src/pipeline/approval.rs:49`), so supplied
digests are store-derived, not shell-supplied; live shell endpoints
(`actions.rs:97–106`, `generate.rs:138–147`) even set `target_digest: None`.

Decision: shell-facing request DTOs structurally cannot carry digest fields, and at
approval-effect time the kernel re-derives the payload digest from artifact-store
bytes (target digest re-derivation already exists at `approval.rs:290`; payload
bytes are re-fetched from the store at `approval.rs:216`). The effect is denied on
any mismatch. `gate()` no longer relies on caller-supplied digest strings; content
integrity rests on the kernel re-hashing bytes, not on trusting stored digests.

## The eight enumerated effect paths

| # | Effect path | File:line | Classification |
|---|-------------|-----------|----------------|
| 1 | `notify_owner_best_effort` | `crates/openspine-kernel/src/pipeline/mod.rs:147–157` (send :154; audit `owner.notified` :150) | `kernel-origin-gated` |
| 2 | `create_approved_draft` | `crates/openspine-kernel/src/pipeline/approval.rs:206–359` (gmail.create_draft :315) | `post-gate-approved-effect` |
| 3 | `activate_approved_artifact` | `crates/openspine-kernel/src/pipeline/approval.rs:367–476` (fs write/rename :447; registry :454) | `post-gate-approved-effect` |
| 4 | `dispatch_read_selected_thread` | `crates/openspine-kernel/src/api/actions.rs:367–441` (gmail.fetch_thread :428; token validation :384–421) | `gated-shell` (token-validated in `gate()`) |
| 5 | `dispatch_lyra_preview` / `propose_draft_creation` | `crates/openspine-kernel/src/api/actions.rs:225–358` (target_digest :333) | `gated-shell` |
| 6 | `dispatch_artifact_propose` | `crates/openspine-kernel/src/api/artifact_propose.rs:37–184` (target_digest :147) | `gated-shell` |
| 7 | `sweep_expired_grants` | `crates/openspine-kernel/src/store/budget_support.rs:94–106` | `internal-maintenance-non-effect` |
| 8 | `answer_callback_query` | `crates/openspine-kernel/src/pipeline/approval.rs:46`; `pipeline/mod.rs:238` | `internal-maintenance-non-effect` |

Classification meaning:
- **`gated-shell`** — shell/agent-initiated, mediated by `gate()` (paths 4–6).
- **`post-gate-approved-effect`** — reached only after a `gate()` Allow; never
  calls `gate()` itself, re-fetches bytes from the kernel store (paths 2–3).
- **`kernel-origin-gated`** — kernel-initiated, routed through `gate()` with
  `Kernel` origin; approval-exempt, audit-never-exempt (path 1).
- **`internal-maintenance-non-effect`** — kernel-internal store/control-plane
  maintenance, no external side effect (paths 7–8).

## Purity split: `gate()` validates, dispatch consumes

`gate()` is pure (no I/O, no mutation) by construction (gate.rs:1–6, 53–60). Two
concerns that previously lived entirely in dispatch are split:

- **Selection-token *validation*** (bound-to-grant, exists, type, expiry) moves
  into pure `gate()` via `GateContext::find_selection_token`. This is a read-only
  lookup that fits `gate()`'s purity.
- **Selection-token *consumption*** (atomic single-use mark) stays at dispatch,
  after `gate()` returns allow — it mutates store state and therefore must not live
  in `gate()`.

This split is stated explicitly in the spec so the purity invariant (D-005 kernel/
shell split, D-007 grant-is-only-live-authority) is preserved while token possession
becomes a `gate()` decision property.

## What does NOT change

- `gate()`'s signature and purity: `pub fn gate(grant, req, ctx, catalog, now) ->
  GateOutcome` (gate.rs:67) stays a pure decision; no I/O is added.
- The three existing `gate()` call sites (`api/actions.rs:108`, `api/generate.rs:
  149`, `pipeline/approval.rs:167`) remain, plus the new `notify_owner_best_effort`
  routing through `gate()` with `Kernel` origin.
- The driver module still does not call `gate()` (`pipeline-driver` requirement
  "The driver MUST NOT invoke gate()"); `notify_owner_best_effort` routes through
  `gate()` at the effect layer (`pipeline/mod.rs`), outside the driver prefix.
- Post-gate effect handlers (`create_approved_draft`, `activate_approved_artifact`)
  keep re-fetching bytes from the kernel store; the change adds kernel-side payload
  digest re-derivation to that pattern.

## Key decisions (D-055)

- **Carve-outs are enumerated catalog data, not implicit paths.** Every effect
  path around `gate()` is a classified, tested catalog entry; the default rule is
  that all shell/agent effects pass through `gate()`, and the only non-shell
  effects are enumerated `kernel-origin-gated` entries routed through `gate()`
  with a `Kernel` origin.
- **`KernelOrigin` exempts from approval, never from audit.** Kernel-initiated
  effects in the enumerated trusted-origin set auto-allow without an approval
  record but always emit `AuditMeta`; anything outside the set is denied.
- **Token validation is a `gate()` property; consumption is dispatch's.** Pure
  `gate()` validates via `find_selection_token`; dispatch performs the atomic
  single-use consume after allow.
- **Digests are kernel-computed, never shell-supplied.** Shell-facing DTOs carry
  no digest fields; the kernel re-derives payload/target digests from artifact-
  store bytes at approval-effect time and denies on mismatch.

## Alternatives considered

- **Keep token validation and digest trust in dispatch (status quo):** rejected —
  it leaves a contract gap where `gate()` decides without knowing token possession
  or digest integrity, relying on "the request happened to be re-read from the
  store." AD-120 and D-004/D-011 require the decision to be made where the
  authority lives.
- **Move token *consumption* into `gate()` too:** rejected — `gate()` is pure
  (no mutation, no I/O); consuming a token would break that invariant and couple
  the decision to store writes. Validation (read-only) belongs in `gate()`;
  consumption (mutating) stays at dispatch.
- **Trust shell-supplied digests but sign them:** rejected — the shell is
  untrusted by construction (D-005/D-010); the kernel must compute outcomes.
  Signing only shifts the trust anchor, it does not remove the shell's ability to
  assert a digest.
- **Make `notify_owner_best_effort` fully gate-mediated (require approval):**
  rejected — D-046 correctly notes gating the trusted kernel against itself adds
  ceremony, not security; the fix is routing it through `gate()` for the *audit*
  event while keeping it approval-exempt.
- **Replace `GateContext::find_selection_token` with a new ad-hoc lookup:** rejected
  — the trait method is already declared and `Store`-implemented; reusing it keeps
  the seam and avoids a second token source of truth.
