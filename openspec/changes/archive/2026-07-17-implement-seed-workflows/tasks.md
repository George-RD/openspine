# Tasks: implement-seed-workflows

## 1. Author the four seed WorkflowManifests
- [x] Create `owner_control_conversation_seed.yaml` (received → composed → replied, no approval).
- [x] Create `email_draft_with_approval_seed.yaml` (selected → drafted → awaiting_approval(approval: email.create_draft) → approved).
- [x] Create `research_and_brief_seed.yaml` (launched → researched → briefed, owner-directed, no approval).
- [x] Create `customer_service_intake_seed.yaml` (received → classified(escalation) → drafted → awaiting_approval(approval: email.create_draft) → approved).

## 2. Embed and materialize seeds as overlay artifacts
- [x] Add `crate::seed_workflows` module embedding the four YAMLs and exposing `all`, `parsed`, `materialize_missing`.
- [x] Wire `seed_workflows::materialize_missing` into `overlay_startup::load` after staged-file discard, before the overlay registry load.
- [x] Declare `mod seed_workflows;` in `main.rs`.

## 3. Verify the acceptance criteria
- [x] `all_seeds_parse_and_validate_as_state_machines`: every seed parses, validates, has an initial state, and all transitions reference declared states.
- [x] `seeds_render_mermaid_flowcharts`: every seed renders `flowchart TD` with one edge per transition.
- [x] `email_draft_seed_declares_digest_bound_approval_state`: the approval-required state binds `email.create_draft`.
- [x] `materialize_is_idempotent_and_preserves_edits`: first boot writes 4 files, subsequent boots write 0, owner edits preserved.
- [x] `seeds_load_as_overlay_namespace_artifacts_on_fresh_install`: after `write_seed_files` + the overlay loader, the four seeds exist on disk and are parsed as overlay workflow artifacts.
- [x] `seeds_register_as_overlay_namespace_learned_artifacts`: driving `materialize_missing` then `overlay_startup::load`, the four seeds are recorded as `Overlay`-namespace learned artifacts (the real registration path behind the `#[cfg(not(test))]` wiring).
- [x] `materialize_runs_once_per_fresh_install`: a persisted marker makes a second boot write 0 and a deleted seed is not re-created.
- [x] `email_seed_approval_state_requires_digest_bound_approval`: leaving `awaiting_approval` is `ApprovalRequired` without a request and succeeds with a matching digest-bound approval.

## 4. Local gate
- [x] `cargo fmt --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`
- [x] `bash scripts/check-file-sizes.sh`
- [x] `openspec validate implement-seed-workflows --strict`
