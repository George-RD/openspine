# Design: Refactor kernel registries

## Approach

Four registration points, each one-to-one with the match-arm or concrete field it replaces. New code lives in new modules (`actions.rs` at 484 lines and `approval.rs` at 480 leave no headroom under the 500-line cap).

### 1. ActionCatalog (schemas) + fail-fast wiring (authority, gate)

- `openspine_schemas::action::ActionCatalog` — an immutable set of known `ActionId`s with `contains(&ActionId)`.
- The kernel curates the canonical list in a new `crates/openspine-kernel/src/action_catalog.rs`: every id referenced by shipped fixtures and dispatch, including intentionally unwired PRD ids. The catalog is built once and shared via `AppState`.
- `compose_authority` takes `&ActionCatalog`; any candidate id (agent tools, workflow/pack/policy action lists) absent from the catalog fails composition with a new structured error variant naming the id and its source artifact. No grant is minted.
- `gate()` takes `&ActionCatalog`; a requested action absent from the catalog is denied with a new `DenialReason::UnknownAction` (distinct from `NotGranted`), audited like every other denial. Known-but-not-granted keeps `NotGranted` exactly as today.

### 2. ActionHandlerRegistry (kernel/api)

- New `crates/openspine-kernel/src/api/handler_registry.rs`: a map from action-id `&'static str` to an async handler fn pointer `for<'a> fn(&'a AppState, &'a TaskGrant, i64, Option<&'a Value>) -> BoxFuture<'a, Result<Value, DispatchError>>`.
- Default registration mirrors today's arms one-to-one: `openspine.status.read`, `telegram.reply:owner_channel`, `email.read_thread:selected_no_attachments`, `lyra.ui.preview`, `artifact.propose` (real); `workflow.invoke:approved`, `setup.workflow.start` (specified stubs).
- Lookup miss ⇒ the existing honest stub (`{stub: true, note}`), never a 500 — an authorized-but-unimplemented action keeps succeeding.
- `email.create_draft` and `artifact.activate` are never registered: approval-gated, reachable only via the approval callback.
- Post-approval resolution (`handle_draft_approval_callback`'s inner match) becomes a second small registry keyed on action id, with the documented default handler = draft creation (every pre-5d approval is a draft); `artifact.activate` is its one non-default entry.

### 3. ConnectorRegistry (kernel)

- New `crates/openspine-kernel/src/connectors.rs`: `trait Connector { fn name(&self) -> &'static str; }` implemented by `TelegramConnector` and `GmailConnector`, and `struct ConnectorRegistry` holding the typed slots (`telegram: TelegramConnector`, `gmail: Option<GmailConnector>`) with typed accessors and an `iter() -> impl Iterator<Item = &dyn Connector>` enumeration seam (the future AD-060/AD-103 registration surface).
- `AppState.telegram` / `AppState.gmail` are replaced by `AppState.connectors`; every call site goes through the registry accessors. Gmail's optionality is preserved bit-for-bit: the `None` branches (approval draft creation, `/draft` selection) are load-bearing graceful degradation.

### 4. Artifact-kind table (kernel)

- A static `ARTIFACT_KIND_SPECS: &[ArtifactKindSpec]` in `artifact_loader.rs` (or a sibling module if the cap demands): `{ name, overlay_subdir, parse: fn(&str) -> Result<ParsedProposal, _>, duplicate_exists: fn(&ArtifactRegistry, &str, u32) -> bool }`.
- The `PROPOSABLE_KINDS` guard, `parse_proposal`'s match, and `dispatch_artifact_propose`'s dup-check match all derive from the one table; the unreachable `_ => false` dup-check arm disappears. `ParsedProposal` stays as the typed carrier (its exhaustive matches are compiler-enforced, not drift-prone).
- Templates are absent from the table — non-proposable stays structural (D-048).

## Key decisions

- **Catalog is a curated kernel const, not derived from fixtures** — deriving from fixtures would make a fixture typo self-legitimizing; the const is the review surface (D-053).
- **Unknown-at-gate is a denial, not an error** — gate stays total over arbitrary shell input; the structured reason distinguishes "outside the universe" from "not granted".
- **Registries are compiled-in** — runtime growth of actions/connectors/kinds stays behind the artifact-lifecycle approval path; this change only relocates where compiled behavior is declared.

## Alternatives considered

- `Box<dyn Connector>` + downcast registry: rejected — typed accessors preserve call-site behavior with zero downcast risk; the dyn enumeration seam still exists for later health/egress work.
- Deleting the honest-stub fall-through since the catalog now validates ids: rejected — the stub covers *known* ids without kernel implementations (e.g. pack-granted `memory.read:owner_preferences_limited`); the catalog covers *unknown* ids. Different layers.
