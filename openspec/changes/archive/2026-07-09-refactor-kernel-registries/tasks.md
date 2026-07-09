# Tasks: Refactor kernel registries

## 1. ActionCatalog + fail-fast

- [x] `ActionCatalog` type in `openspine-schemas` (`action.rs`): immutable known-id set, `contains`.
- [x] Curated canonical list in `crates/openspine-kernel/src/action_catalog.rs` covering every fixture-referenced id, including unwired PRD ids (`route.activate`, `workflow.activate`, `capability_pack.change`, `policy.change_proposal`, `connector.enable`); shared via `AppState`.
- [x] `compose_authority(&input, &catalog, now)`: structured `UnknownActionId { id, source }` error for any candidate id outside the catalog; both kernel call sites updated.
- [x] `gate()` denies catalog-unknown ids with new `DenialReason::UnknownAction`, audited; `NotGranted` unchanged for known-but-not-granted.
- [x] Tests: composition rejects an unknown fixture id (test-only fixture); gate denies an unknown id with `UnknownAction`; gate keeps `NotGranted` for a known ungranted id; catalog covers all shipped fixture ids (exhaustiveness test that loads the fixture registry and validates every referenced id).

## 2. ConnectorRegistry

- [x] `connectors.rs`: `Connector` trait (`name()`), impls for `TelegramConnector`/`GmailConnector`, `ConnectorRegistry` with typed accessors + `iter()`.
- [x] `AppState.telegram`/`AppState.gmail` → `AppState.connectors`; update `main.rs`, `test_support.rs` (including `test_state_with_telegram`/`test_state_with_gmail`), and every call site (`api/actions.rs`, `api/artifact_propose.rs`, `pipeline/mod.rs`, `pipeline/approval.rs`, `pipeline/selection.rs`).
- [x] Gmail `None` graceful-degradation branches preserved verbatim (approval `create_approved_draft`, selection `handle_thread_selection`).
- [x] Test: registry `iter()` enumerates configured connectors; existing connector-behavior tests pass unchanged.

## 3. ActionHandlerRegistry

- [x] `api/handler_registry.rs`: handler fn-pointer map; default registration mirroring today's seven arms one-to-one; lookup miss ⇒ existing honest stub.
- [x] `dispatch_allowed_action` becomes a registry lookup; `email.create_draft`/`artifact.activate` never registered.
- [x] Post-approval resolution registry: `artifact.activate` entry + documented draft-creation default; `handle_draft_approval_callback` uses it.
- [x] Tests: registered real action dispatches; unregistered-but-known action returns the stub shape; approval fall-through still routes a non-`artifact.activate` approval to draft creation.

## 4. Artifact-kind table

- [x] `ArtifactKindSpec` static table (name, overlay_subdir, parse, duplicate_exists) as single source of truth.
- [x] `PROPOSABLE_KINDS` guard, `parse_proposal`, and the propose dup-check derive from the table; unreachable `_ => false` arm removed.
- [x] Templates absent from the table; `artifact_propose_rejects_template_kind` still passes.
- [x] Test: table-driven parse/dup-check round-trips for all five kinds.

## 5. Decision log + docs

- [x] Add D-053 (curated canonical ActionCatalog; unknown-at-composition hard error; unknown-at-gate structured denial distinct from `NotGranted`) to `.raw/openspine-decision-log.md` with index + changelog rows.
- [x] `graphify update .` after code changes (pre-commit hook re-runs it).

## 6. Validation

- [x] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`.
- [x] `cargo test --workspace` — all pre-existing tests pass unchanged in meaning.
- [x] `bash scripts/check-file-sizes.sh` — all files ≤500 lines.
- [x] `openspec validate refactor-kernel-registries --strict` and `./scripts/check.sh` green (12 passed, 230 workspace tests).
- [x] Independent reviewer subagent pass on the diff before commit (correctness/security + simplicity/scope lenses); the one blocking finding (catalog check ordered after grant lists at gate) fixed with a stale-grant regression test.
