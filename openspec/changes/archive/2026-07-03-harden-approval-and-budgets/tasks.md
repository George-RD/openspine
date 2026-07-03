# Tasks: Harden approval and budgets

## 1. WYSIWYS (2a)

- [x] Build the full preview text before truncating it for Telegram.
- [x] Skip `propose_draft_creation` entirely when the shown text was truncated.
- [x] Send a truncation notice that always fits under Telegram's UTF-16 limit.
- [x] Audit `draft.proposal_failed` with reason `preview_truncated`.
- [x] Test: a truncated preview carries no approval button and persists no `ActionRequest`.

## 2. Enforce `max_model_calls` (2b)

- [x] Add `Store::count_conversation_turns`.
- [x] Check the budget in `post_model_generate` before the payload put and the new user turn.
- [x] Deny `limit_exceeded` with no provider call once the budget is spent.
- [x] Test: `max_model_calls: 1` allows exactly one call, denies the second with zero further provider hits.

## 3. Enforce `max_artifacts` (2c)

- [x] New `grant_counters` table.
- [x] Add `Store::try_count_artifact_put` (one atomic SQL statement, TOCTOU-safe).
- [x] Wire the check into `model.generate`'s payload put and `propose_draft_creation`'s payload put.
- [x] Test: `max_artifacts: 1` allows exactly one shell-initiated put, denies the second.

## 4. Hash task tokens at rest; sweep expired grants (2d)

- [x] `insert_task_grant` stores `hash(token)`, not the plaintext.
- [x] `find_task_grant_by_token` hashes its input before lookup.
- [x] Redact `task_token` from the persisted `grant_json` blob.
- [x] `Store::sweep_expired_grants`, called at the top of `insert_task_grant`.
- [x] Confirmed no call site reads `.task_token` off a loaded/persisted grant
      (`grep -rn "\.task_token" crates/` — every remaining site reads a
      freshly-minted, pre-persist grant).
      Note: existing dev databases need no migration — plaintext rows simply
      stop matching once this ships, and task tokens expire in ≤180s anyway.
- [x] Tests: `find_task_grant_by_token_rejects_the_raw_hash_value`,
      `persisted_grant_json_contains_no_task_token`,
      `sweep_removes_only_grants_expired_more_than_a_day`.

## 5. Audit the trusted notification path (2e)

- [x] `notify_owner_best_effort` appends a best-effort `owner.notified` audit row.

## 6. Operator docs (2f)

- [x] `.env.example`: Gmail section (`OPENSPINE_GMAIL_CLIENT_SECRET`, `OPENSPINE_GMAIL_REFRESH_TOKEN`).
- [x] `compose.yaml`: commented-out `docker-socket-proxy` service, inactive by default.
- [x] README "Threat notes": reference the socket-proxy option.

## 7. Spec deltas (2g)

- [x] `digest-bound-draft-approval`: approval must bind only what the owner
      was shown; approvals must expire.
- [x] `gate-action-api`: grant limits must be enforced at runtime;
      kernel-originated owner notifications are a trusted, audited path.
- [x] `selected-thread-email-preview-slice`: selection tokens must be
      single-use, expiring, and scope-fixed.

## 8. Decision log (2h)

- [x] D-045 (WYSIWYS), D-046 (budget enforcement placement), D-047 (token
      hashing and sweep) appended to `.raw/openspine-decision-log.md`.

## 9. Validation

- [x] `cargo test --workspace` all green.
- [x] `scripts/check-file-sizes.sh` green (the truncation helpers were
      split into `api/telegram_truncate.rs` to keep `actions.rs` under
      the 500-line gate).
- [x] `npx --no-install openspec validate harden-approval-and-budgets --strict`.
