# Tasks: Refactor pipeline driver

## 1. Typed stage sequence

- [ ] `PipelineStage` enum in `crates/openspine-kernel/src/pipeline/driver.rs`: nine variants, `PipelineStage::SEQUENCE` (canonical order, declared once) and `PipelineStage::SYNC_PREFIX` derived from `SEQUENCE` (truncated before `Gate`).
- [ ] Driver execution derived from `SYNC_PREFIX`: iterate the prefix, dispatch each stage through one `match` — the enum is the executable stage plan.
- [ ] Tests: pin `SEQUENCE` contents/order; instrumented driver run's executed-stage trace equals `SYNC_PREFIX` for both lanes.

## 2. LaneSpec + lane constructors

- [ ] `LaneSpec` record capturing the full divergence table: `channel_trust`, `lane`, authority `purpose`, envelope construction, lane preflight verification, selection-token minting + grant binding, pending task input (raw_ref vs derived pending_ref), target.
- [ ] Hook contract enforced by construction: hooks are single-stage typed adapters; no hook calls `resolve_route`, `compose_authority`, `insert_task_grant`, or `run_task`; no cross-stage audit emission; no hook invokes another hook or stage.
- [ ] `owner_control_lane()` and `email_preview_lane()` compiled-in constructors beside the driver; no runtime registration/mutation/removal path.
- [ ] `LaneSpec` carries no sequencing capability (no stage list, no ordering field).

## 3. Driver + cutover

- [ ] Single driver entry interpreting a `LaneSpec`; driver alone owns stage dispatch, early-return handling, `event.received` emission (after Verify succeeds), grant persistence, and shell run; per-stage audit emission preserved verbatim (names, metadata, order).
- [ ] `run_telegram_poll_loop` hands updates to the driver; `/draft` detection becomes lane selection at the driver boundary (Event stage).
- [ ] Preflight-failure exits preserved verbatim: `selection.gmail_not_configured`, `route.refused_uncontained`, `selection.thread_not_found`, `selection.gmail_error` audits + owner notifications, with no `event.received` on any of them.
- [ ] Cutover, not accretion: `handle_owner_update` and `handle_thread_selection` bodies deleted; `pipeline/selection.rs` deleted or reduced to live selection-specific helpers only; no wrappers, aliases, or re-exports; `pipeline/mod.rs` shrinks below the cap because code moved or died.
- [ ] Untouched: gate call sites (`api/actions.rs`, `api/generate.rs`, `pipeline/approval.rs`), `notify_owner_best_effort`, `answer_callback_query`, poll-loop offset persistence, `handle_draft_approval_callback` + post-approval resolution. Driver module never imports/calls `gate()`.

## 4. Tests

- [ ] All existing pipeline/API/authority/gate tests pass unchanged in meaning (mechanical call-site updates only, if any).
- [ ] New: stage-order pin; executed-stage trace == `SYNC_PREFIX` (both lanes); no `event.received` on each of the four `/draft` preflight-failure paths; `GET /v1/task.pending_message` and `authority.granted` refs pinned for both lanes (owner raw_ref; email derived pending_ref).
- [ ] Driver module contains no `gate()` import/call (structural assertion or review-checked); gate-mediated dispatch tests remain owned by gate-action-api and continue passing.

## 5. Decision log + docs

- [ ] Add D-054 (typed compiled-in stage sequence the driver executes; lanes as compiled-in data records with a single-stage hook contract; gate as distributed runtime stage outside the driver prefix; lanes cannot reorder/omit stages; `event.received` post-Verify) to `.raw/openspine-decision-log.md` with index + changelog rows.
- [ ] `graphify update .` after code changes.

## 6. Validation

- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- [ ] `bash scripts/check-file-sizes.sh` — all files ≤500 lines.
- [ ] `openspec validate refactor-pipeline-driver --strict` and `./scripts/check.sh` green.
- [ ] Independent reviewer subagent pass on the diff before commit.
