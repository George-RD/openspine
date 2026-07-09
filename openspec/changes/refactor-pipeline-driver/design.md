# Design: Refactor pipeline driver

## Approach

One driver, two lane records, no behavior change. New code lives in the existing `pipeline` module; `pipeline/mod.rs` (478 lines) leaves no headroom under the 500-line cap, so the driver and lane specs land in new sibling files and the old flow bodies are deleted, not wrapped.

### 1. Typed stage sequence — consumed, not decorative

- `PipelineStage` enum in `crates/openspine-kernel/src/pipeline/driver.rs`: nine variants — `Event`, `Verify`, `Identify`, `Route`, `Compose`, `Grant`, `Run`, `Gate`, `Audit` — with the canonical order declared once as `PipelineStage::SEQUENCE` and the synchronous prefix derived from it as `PipelineStage::SYNC_PREFIX` (`SEQUENCE` truncated before `Gate`).
- The driver's execution is derived from `SYNC_PREFIX`: it iterates the prefix and dispatches each stage through one `match`, so the enum is the executable stage plan, not documentation. Tests assert (a) `SEQUENCE` pins the nine stages in canonical order, and (b) an instrumented driver run's executed-stage trace equals `SYNC_PREFIX` — for both lanes.
- Stage semantics, pinned to current behavior:
  - `Event` = raw intake and lane selection (poll projection, `/draft` command detection). Unaudited, exactly as today.
  - `Verify` = owner/source verification plus lane preflight (for the email preview lane: Gmail configured, containment guard, `thread_exists`). All preflight failure exits keep their current audit events (`selection.gmail_not_configured`, `route.refused_uncontained`, `selection.thread_not_found`, `selection.gmail_error`) and owner notifications.
  - The audited event envelope (`event.received`) is emitted by the driver after `Verify` succeeds — which is where both flows emit it today (owner flow: `pipeline/mod.rs:272-273`; selection flow: `selection.rs:164-199` after preflight). No `event.received` is ever emitted on a preflight-failure path, matching current behavior; tests pin this for all four `/draft` preflight failures.
  - `Identify`→`Route`→`Compose`→`Grant`→`Run` map one-to-one onto the existing calls (`resolve_owner_identity`, `resolve_route`, `compose_authority`, `insert_task_grant` + `authority.granted` audit, `Sandbox::run_task`).
- `Gate` and `Audit` appear in `SEQUENCE` (the type names the whole pipeline honestly) but are documented as distributed: `Gate` executes at the shell dispatch surface and the approval callback; `Audit` is woven through every stage.

### 2. Lanes as data — with a hard hook boundary

- `LaneSpec` captures the exhaustive divergence between the two current flows:
  - `channel_trust: ChannelTrust` — `VerifiedOwnerChannel` (owner-control) vs `OwnerDevice` (email preview).
  - `lane: Lane` — `OwnerControl` vs `ExternalCommunication`; the containment guard consumes this, so guard behavior is lane-driven (structural no-op for owner-control, load-bearing for external-communication, as today).
  - authority `purpose` — `"owner_control_conversation"` vs `"selected_thread_email_reply_draft"`.
  - envelope construction — owner text envelope (`build_owner_envelope`) vs inline Gmail `EmailThreadSelected` envelope.
  - lane preflight verification — none vs Gmail-configured + containment + `thread_exists` (with the existing degradation branches).
  - selection-token minting and grant binding — none vs mint + `insert_selection_token` + bind `grant.selection_tokens`.
  - pending task input — owner flow persists the original message `raw_ref`; email lane builds the derived pending message ("Draft a reply to Gmail thread … (selection token …)"), persists `pending_ref`, and that ref is what `authority.granted` audits and `GET /v1/task` returns as `pending_message`. Tests pin `pending_message` and the `authority.granted` refs for both lanes.
  - target — `main_assistant_agent` vs `email_reply_drafter`, resolved through `resolve_route` exactly as today.
- **Hook contract (hard boundary).** Where a variation point is behavior, the `LaneSpec` field is a single-stage typed adapter: typed inputs in, typed outputs out. A hook MUST NOT call `resolve_route`, `compose_authority`, `insert_task_grant`, or `run_task`; MUST NOT emit audit for any stage other than its own existing events; and MUST NOT invoke another hook or stage. The driver alone owns stage dispatch, early-return handling, `event.received` emission, grant persistence, and the shell run. A lane hook that reimplemented the old `handle_thread_selection` body behind one closure would violate this contract and fail review.
- The two lane constructors live beside the driver (`owner_control_lane()`, `email_preview_lane()`); they are compiled-in kernel values with no runtime registration, mutation, or removal path. `run_telegram_poll_loop` keeps its intake role and hands updates to the single driver entry point; `/draft` detection becomes lane selection at the driver boundary (`Event` stage) instead of a branch buried mid-driver.

### 3. Cutover — code moves, it does not accrete

- `handle_owner_update` and `handle_thread_selection` bodies are deleted, not wrapped: `pipeline/mod.rs` loses the duplicated stage prefix outright, and `pipeline/selection.rs` is deleted or retains only live selection-specific helpers (token construction, pending-message formatting) that the email lane's hooks call.
- The file-size gate must pass because code moved or died — not because new files were added around dead weight. No transitional wrappers, aliases, or re-exports remain.

### 4. What does NOT move

- `gate()` call sites: `api/actions.rs`, `api/generate.rs`, `pipeline/approval.rs` — untouched. The driver module never imports or calls `gate()`; that structural boundary is this change's requirement, while gate mediation semantics remain owned by the gate-action-api capability and its existing tests.
- `notify_owner_best_effort` — the kernel-originated owner-notification path (trusted, audited as `owner.notified`, not gate-mediated, per the gate-action-api requirement "Kernel-originated owner notifications are a trusted, audited path"), `answer_callback_query`, and the poll-loop offset persistence — untouched effectful paths outside the stage prefix.
- All audit event names, metadata, and per-stage emission points — the refactor relocates the code that emits them, not what or when they emit.
- `handle_draft_approval_callback` and post-approval resolution — unchanged gate-stage runtime path; it is not a third lane.

## Key decisions (D-054)

- **Stages are a typed compiled-in sequence the driver executes; lanes are compiled-in data records.** Canon never fixed the representation. A runtime-proposable lane artifact would let approved YAML alter verification order — authority-sensitive machinery this behavior-preserving change must not introduce. Runtime lane growth, if ever wanted, goes through the artifact-lifecycle approval path as its own change.
- **Gate is a distributed runtime stage, not a driver step.** The nine-stage listing puts gate after run because effects happen when the shell dispatches intents; the kernel gates each intent at the effect boundary (AD-120, D-004). The driver type names gate so the sequence is honest, but execution stays at the dispatch surface, and the driver module never calls `gate()`.
- **Lanes cannot reorder or omit stages.** The driver owns the order via `SYNC_PREFIX`; a `LaneSpec` carries no sequencing capability. Per-lane "skips" (owner-control has no preflight verification) are expressed as no-op inputs to that stage, so the stage still runs in order.
- **`event.received` is post-Verify.** Both shipped flows emit the audited envelope only after verification succeeds; the driver pins that placement, so preflight failures never emit `event.received` — exactly today's audit surface.

## Alternatives considered

- **Trait-object `Stage` pipeline (`Vec<Box<dyn Stage>>`)**: rejected — dynamic stage composition is exactly what "lanes cannot reorder stages" forbids; it buys indirection and allocation for a sequence that is fixed by canon.
- **Lane as a `match` on `Lane` inside one merged function**: rejected — that is the current design with the duplication folded in; adding a lane would still mean editing driver internals rather than adding data.
- **Full state-machine driver with resumable per-stage checkpoints**: rejected — that is `implement-durable-workflow-replay` (AD-104) territory; nothing in this change's canon requires persistence of stage progress.
- **`event.received` emitted at the top of the driver (stage-literal reading)**: rejected — it would add `event.received` to the four `/draft` preflight-failure paths and change the audit surface; behavior preservation wins over stage-label aesthetics.
