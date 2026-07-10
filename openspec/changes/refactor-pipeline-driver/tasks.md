# Tasks: Refactor pipeline driver

## 1. Typed stage sequence

- [x] `PipelineStage` enum in `crates/openspine-kernel/src/pipeline/driver.rs`: nine variants, `PipelineStage::SEQUENCE` (canonical order, declared once) and `PipelineStage::SYNC_PREFIX` derived element-by-element from `SEQUENCE` (truncated before `Gate`), with a const assertion pinning `Gate`/`Audit` as the tail.
- [x] Driver execution checked against `SYNC_PREFIX`: `run_pipeline` records an instrumented stage trace; the trace-equality tests hold the driver to the declared plan.
- [x] Tests: pin `SEQUENCE` contents/order; instrumented driver run's executed-stage trace equals `SYNC_PREFIX` for both lanes.

## 2. LaneSpec + lane constructors

- [x] `LaneSpec` record capturing the full divergence table: `channel_trust`, `lane`, authority `purpose`, envelope construction, lane preflight verification, selection-token minting + grant binding, pending task input (raw_ref vs derived pending_ref), target.
- [x] Hook contract enforced by construction: hooks are single-stage typed adapters; no hook calls `resolve_route`, `compose_authority`, `insert_task_grant`, or `run_task`; no cross-stage audit emission; no hook invokes another hook or stage.
- [x] `owner_control_lane()` and `email_preview_lane()` compiled-in constructors beside the driver; no runtime registration/mutation/removal path.
- [x] `LaneSpec` carries no sequencing capability (no stage list, no ordering field).

## 3. Driver + cutover

- [x] Single driver entry interpreting a `LaneSpec`; driver alone owns stage dispatch, early-return handling, `event.received` emission (after Verify succeeds), grant persistence, and shell run; per-stage audit emission preserved verbatim (names, metadata, order).
- [x] `run_telegram_poll_loop` hands updates to the driver; `/draft` detection becomes lane selection at the driver boundary (Event stage).
- [x] Preflight-failure exits preserved verbatim: `selection.gmail_not_configured`, `route.refused_uncontained`, `selection.thread_not_found`, `selection.gmail_error` audits + owner notifications, with no `event.received` on any of them.
- [x] Cutover, not accretion: `handle_thread_selection` deleted outright; `handle_owner_update` retained as the intake/lane-selection entry point with its old stage-prefix body replaced by driver delegation; `pipeline/selection.rs` reduced to live selection-specific helpers only; no wrappers, aliases, or re-exports; `pipeline/mod.rs` shrinks below the cap because code moved or died.
- [x] Untouched: gate call sites (`api/actions.rs`, `api/generate.rs`, `pipeline/approval.rs`), `notify_owner_best_effort`, `answer_callback_query`, poll-loop offset persistence, `handle_draft_approval_callback` + post-approval resolution. Driver module never imports/calls `gate()`.

## 4. Tests

- [x] All existing pipeline/API/authority/gate tests pass, unchanged in meaning except one deliberate wave-1 correction: `draft_command_for_a_missing_thread_mints_no_grant` gains the containment opt-in so it genuinely reaches `selection.thread_not_found` (previously it exited at the containment guard despite its name).
- [x] New: stage-order pin; executed-stage trace == `SYNC_PREFIX` (both lanes); no `event.received` on each of the four `/draft` preflight-failure paths; pending task input, `grant.purpose`, and `authority.granted` refs pinned for both lanes (owner raw_ref; email derived pending_ref).
- [x] Driver module contains no `gate()` import/call (structural assertion or review-checked); gate-mediated dispatch tests remain owned by gate-action-api and continue passing.

## 5. Decision log + docs

- [x] Add D-054 (typed compiled-in stage sequence the driver executes; lanes as compiled-in data records with a single-stage hook contract; gate as distributed runtime stage outside the driver prefix; lanes cannot reorder/omit stages; `event.received` post-Verify) to `.raw/openspine-decision-log.md` with index + changelog rows.
- [x] `graphify update .` after code changes.

## 6. Validation

- [x] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- [x] `bash scripts/check-file-sizes.sh` — all files ≤500 lines.
- [x] `openspec validate refactor-pipeline-driver --strict` and `./scripts/check.sh` green.
- [x] Independent reviewer subagent pass on the diff before commit (behavior-preservation + security/spec-conformance lenses; both APPROVE — findings fixed: `grant.purpose` pins added, D-054 iterate claim corrected, cutover/test wording made honest, email-lane timestamp deviation documented).
