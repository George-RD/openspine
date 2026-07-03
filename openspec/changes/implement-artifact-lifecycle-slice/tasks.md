# Tasks: Implement artifact lifecycle slice

## 1. Registry & schema plumbing

- [x] Wrap `ArtifactRegistry` in a lock (`AppState.registry`); update every read site to take a guard, never held across an `.await`.
- [x] Split `load_registry` into `load_registry_into` (merge target) + a thin `load_registry(dir)` wrapper.
- [x] Load the `data/artifacts.d` overlay at startup, after the fixture tree; fail startup on an id collision between fixture and overlay.

## 2. Store

- [x] `proposed_artifacts` table + `insert_proposed_artifact` / `find_proposed_artifact_by_action_request` / `proposed_artifact_exists` / `set_proposed_artifact_state`.
- [x] `set_proposed_artifact_state` enforces lifecycle legality via `openspine_schemas::artifact::can_transition` before the UPDATE.

## 3. Kernel: `artifact.propose`

- [x] Replace the stub with `dispatch_artifact_propose`: payload contract, budget check, per-kind schema parse, `lifecycle_state: proposed` requirement, id+version uniqueness (registry + pending proposals), persist YAML + proposed row, mint digest-bound `artifact.activate` approval request, send the approval button.

## 4. Kernel: approval branch + activation

- [x] `handle_draft_approval_callback` branches on `request.action.as_str()`; `email.create_draft` path unchanged.
- [x] `activate_approved_artifact`: re-parse the approved YAML, flip `lifecycle_state` to `active`, write-to-temp-then-rename into the overlay, insert into the live registry under a write guard, audit `artifact.activated`, notify the owner.

## 5. Fixtures + composition

- [x] Add `artifact.activate` to `owner_control_basic_pack.yaml`'s `approval_required`; confirm `artifact.propose` is already a candidate allowed action.
- [x] Update `owner_control_grant_matches_prd_12_1` for the new approval-required entry.

## 6. Shell: `/propose` UX

- [x] `cmd_propose` splits `/propose <kind>\n<yaml>`; missing kind or empty body replies usage text without calling the kernel.
- [x] Update `propose_command_sends_correct_payload`; add a usage-reply test for the empty-body case.

## 7. Tests

- [x] `artifact_propose_persists_and_sends_approval_button`.
- [x] `artifact_propose_rejects_malformed_yaml`.
- [x] `artifact_propose_rejects_unknown_kind`.
- [x] `artifact_propose_rejects_duplicate_id_version`.
- [x] `artifact_propose_rejects_non_proposed_lifecycle`.
- [x] `approved_artifact_activates_into_registry_and_overlay` (full propose → approve → activate flow; asserts both the in-memory registry and the on-disk overlay file).
- [x] `activation_with_mutated_payload_is_denied` (a second identical proposal for an already-active id/version is rejected — digest-mismatch denial itself is already covered by `approved_but_payload_changed_since_is_denied_not_reasked`, not re-derived here).
- [x] `artifact_propose_rejects_template_kind` (prompt templates are not a proposable kind).

## 8. Validation

- [x] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` — both clean (no `await_holding_lock` on the new registry lock).
- [x] `cargo test --workspace` — 218 passed, 0 failed; all new tests verified passing individually and mutation-tested to confirm they fail when the guard they assert is removed.
- [x] `npx --no-install openspec validate implement-artifact-lifecycle-slice --strict` — valid.
- [x] `./scripts/check.sh implement-artifact-lifecycle-slice` — fully green.
