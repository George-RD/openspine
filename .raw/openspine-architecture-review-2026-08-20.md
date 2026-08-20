# Architecture review — 2026-08-20

Four read-only scout passes over `crates/` (effect flow, owner surface and
identity, store and pipeline composition, test seams), plus a merged external
tooling review (Graphify → CodeGraph). Vocabulary: deep/shallow module,
interface, implementation, seam, adapter, leverage, locality (the
codebase-design skill). Verdicts applied per the decision test in
[DIRECTION.md](../DIRECTION.md); users and promises per
[CONTEXT.md](../CONTEXT.md) / [STORIES.md](../STORIES.md).

## Candidate 1 — One deep settlement module for effect disposition

**Verdict: land → fold into the Effect Truth track (#173/#174).**
Users: Lyra, Bell, Unattended workhorse, Auditor. Promise: Ledger.

- `api/actions.rs` (1549 lines): `mediate_and_dispatch_action_with_attribution_and_token`
  is ~940 lines (373–1311) with settlement policy inline; the wrapper family
  grows to 8 args plus a headless twin.
- A typed `EffectOutcome` (Executed / RefusedPreEffect / DeliveryUnknown /
  FailedAfterAttempt) already exists at the executor seam
  (`api/effect_executors.rs`, one registration) — and is re-collapsed into
  `DispatchError`/anyhow at `api/scoped_admission.rs:526-539`, forcing
  caller-side settlement interpretation.
- Three parallel string-keyed fan-outs on one path: `ActionHandlerRegistry`
  (15 keys), `EffectExecutorRegistry` (1), `POST_APPROVAL_HANDLERS`.
- Deepening direction: disposition classified once at the connector seam;
  settlement (finalize / cancel / retain+fence) and audit-kind selection
  decided inside one module; registries concentrated. A WhatsApp connector
  reuses the same seam.

## Candidate 2 — Deepen the Store: transaction/audit discipline inside

**Verdict: land → design ticket (new; nothing on the roadmap carries it).**
Users: Auditor, Unattended workhorse, Lyra. Promise: Ledger.

- The widest interface in the workspace: ~150 pub methods across 40
  `impl Store` blocks in 40 files, 186 dependent files, `StoreError` with 37
  variants, `pub(crate) conn`.
- BEGIN IMMEDIATE re-stated at ~40 sites (37 `transaction_with_behavior`
  + 3 raw `execute_batch`), one *outside* the store module
  (`skill/review.rs:155` opens its own transaction on `store.conn`).
- Audit-before-effect pairing enforced by convention at ~49 `.append_audit(`
  sites plus ~25 `append_audit_conn(&tx, …)` pairings
  (`store/audit_append.rs` exports the discipline instead of encapsulating
  it).
- The store reaches *up* into `telegram::` (`store/worker_dispatch.rs:255`).
- Deepening direction: the two invariants move inside the implementation so
  callers can't restate or skip them; `conn` goes private; a wrong ordering
  stops compiling. Load-bearing for #173/#174 settlement writes and for
  tenancy audit dimensions later.

## Candidate 3 — Owner surface: finish the half-built seam

**Verdict: land → fold into #129 (owner-surface track).**
Users: Lyra, Bell. Promise: Switchboard.

- `OwnerSurfaceRef` is a real seam on the review path (two adapters, shared
  `render_owner_review`); everything else is parallel implementations.
- ~28 Telegram chat-id leaks below the seam, including store DDL
  (`store/failure_surfacing_types.rs` persists `chat_id INTEGER`), the
  OwnerControl envelope (`pipeline/lanes.rs:191`), and kernel-origin delivery
  hardwired in ~10 places (headless, standing-rule timers, timer dispatch,
  nerve delivery, retry worker, main.rs).
- Kind-dispatch triplicated: `telegram.rs:55`,
  `message_notify.rs:261-296`, `owner_review_decision.rs:267/430`.
- Five command families are Telegram-only (`/secret`, `/disclosure`,
  `/bind`, `/digest`, `/skill install`); a terminal owner cannot answer a
  disclosure question. `handle_owner_update` is 586 lines, entry type
  hard-coded to `&telegram::TelegramUpdate`.
- Deepening direction: channel-neutral command surface above the seam
  (`owner_review_commands.rs` is the proven template); delivery,
  kind-dispatch, and dead-letter concentrated behind per-channel adapters.

## Candidate 4 — One typed owner identity

**Verdict: land → design ticket (new). Decided: design precedes the tenancy
assessment.** Users: Auditor, Bell, Lyra. Promise: Permissions.

- Three parallel owner scalars on AppState (`pipeline/mod.rs:145-147`) plus
  stringly `TaskGrant.user` (`schemas/grant.rs:40`) carrying four value
  shapes: stringified principal Ulid (`authority/compose.rs:296`), `"owner"`
  literals, raw Telegram id, `"kernel"`. ~35 read/write sites.
- Live defect: `ApprovalRecord.approved_by` persists a raw Telegram user id
  as the approver of record (`pipeline/approval.rs:204`,
  `plan_approval.rs:179`).
- Audit has no identity dimension; owner facts ride in reason strings.
  `tests/driver.rs:88-91` asserts `grant.user != owner_user_id` — the code
  documents its own confusion.
- Deepening direction: one typed owner-identity module, selection happens
  once; the grant carries a typed kernel-owned principal reference (the
  MAC-covered `thread_id` precedent); the approver of record derives from the
  verified surface; identity becomes an audit dimension.

## Candidate 5 — One fault seam, one test-support home

**Verdict: land → fold into #177.** Correctness/test shape; no story needed.

- 13 fault-injection mechanisms across 8 files, three naming schemes, two arm
  semantics. Four flags are compiled into release builds
  (`fail_next_owner_reconfirmation`, `fail_next_standing_rule_remaining`,
  `fail_next_effective_allow_audit`, `fail_next_reservation_cancel`) — atomic
  loads on the production path; the seam must be test-gated wholesale.
- ~9 test files hand-write raw SQL through `pub(crate) conn` for crash-state
  setup; blob/key path layout re-derived in artifact-store tests.
- Shell-E2E harness duplicated (`worker_e2e_tests.rs` vs
  `pipeline/tests/terminal_e2e.rs`); de-facto fixture home is
  `api/dispatch_tests.rs` (imported by ~15 files), not `test_support.rs`.

## Candidate 6 — Make the tested path the production path (polling twin)

**Verdict: land → file as an implementation-ready issue.**
User: Unattended workhorse (replay firing twice); correctness needs no story.

- `pipeline/polling.rs:78-91` `poll_telegram_once_for_test` is a verbatim
  copy of the production loop body (33–62); the error branch, backoff, and
  at-most-once ordering live only in the untested loop. `cfg(test)`
  re-exports at `pipeline/mod.rs:72-74`.
- Direction: one poll-once implementation; the loop becomes a thin driver;
  the twin and re-exports are deleted.

## Candidate 7 — Delete the dead seam; name the reservation

**Verdict: land → fold into the same Effect Truth comment as candidate 1.**
Correctness/hygiene; no story needed.

- `openspine-gate/src/gate.rs`: `_egress: &dyn EgressClassifier` has zero
  production callers — a zero-adapter seam on the purest module in the
  workspace; the gate resolves egress from the catalog itself.
- The reservation is a bare `(String, u32, String)` tuple re-declared in four
  modules (`disclosure.rs`, `api/actions.rs`, `api/scoped_admission.rs`,
  `standing_rules_fired_token.rs`); it is the central object of effect
  settlement and has no home.

## Merged external review — Graphify → CodeGraph

- Already landed (commit `aee68fc`): codegraph MCP in `.omp/mcp.json`,
  `.codegraph/` gitignored, `graphify-out/` gone from the tree.
- Acceptance queries verified live: impact of `ResolvedActionContext` = 20
  callers (schemas `responsibility.rs`/`reviewed_scope.rs`, store
  scoped-rules, scoped admission, 5 test suites). `ActionDisposition` does
  not exist yet (T17 vocabulary; T2/T3 settle `EffectDisposition`).
- History finding, measured: committed graphify snapshots = **41.1 MiB across
  4,740 objects ≈ 54 % of the repo's 76 MiB object storage** (`.git` 138 MB
  on disk). Ruling: rewrite decided, owner-executed (DIRECTION.md).
- Convergence: god-modules blunt the graph — a 940-line function is one node.
  The deepening candidates are what make callers/impact queries sharp.

## Not carded (fog)

Surfaced by scouts, unscoped; revisit after settlement and owner-surface
decisions land:

- `main.rs` (1097 lines): hand-wired startup, 10-subsystem `tokio::select!`,
  two inline recovery loops duplicating a ~30-line notification body.
- `pipeline/owner_review_decision.rs`: ~800-line decision fn where a
  decision-intent state machine belongs.
- `api/worker.rs`: worker lifecycle flattened into HTTP handlers; the state
  machine half-exists in `store/worker_dispatch.rs`.
