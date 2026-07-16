# Tasks

- [x] Define versioned `Plan` and `PlanStep` payloads with exact execution identity.
- [x] Compute the complete serialized plan digest through canonical JSON.
- [x] Render every digest-bound field in the owner-facing question.
- [x] Persist canonical plan bytes in the existing pending ActionRequest flow.
- [x] Add verified `approve_plan:<id>` callback routing and atomic consumption.
- [x] Re-derive the plan digest from artifact-store bytes before approval persistence.
- [x] Reuse ApprovalRecord and gate payload-digest enforcement.
- [x] Resolve approved plans by auditable announcement without executing steps.
- [x] Refuse approval affordances when complete Telegram rendering truncates.
- [x] Add schema, gate, callback, and kernel-path tests for mutation refusal.
- [x] Run formatting, lint, workspace tests, file-size, and strict OpenSpec validation.

The first plan producer and individual step execution semantics are intentionally deferred to `implement-workflow-state-machines` / `implement-worker-runtime`.
