# Tasks: Harden gate trusted paths

> Spec-only change directory. This file plans the implementation; the Rust edits,
> threat-claims register rows, and decision-log D-055 entry listed below are
> **implementation tasks** and are NOT performed by this change directory
> (non-goals: no Rust changes, no threat-claims edits, no decision-log edits here).

## 1. gate crate — pure `gate()` changes

- [x] Add `ActionOrigin::{Shell, Kernel}` to the gate types; `gate()` takes an
  origin (or resolves it from the request/catalog) and threads it into `AuditMeta`.
- [x] For catalog-marked `token_requiring` actions, call `GateContext::
  find_selection_token` (gate.rs:35; impl `api/mod.rs:65–76`) inside the pure
  decision and deny when the token is not bound to the requesting grant, missing,
  wrong type, or expired.
- [x] For `ActionOrigin::Kernel` requests whose action is in the enumerated
  trusted-origin set, auto-allow (exempt from approval) but always emit
  `AuditMeta`; deny kernel-origin requests for actions outside the set.
- [x] Keep `gate()` pure: no I/O, no mutation; the atomic token consume remains at
  dispatch, not in `gate()`.

## 2. ActionCatalog metadata

- [x] Add an enumerated kernel-origin action set to `ActionCatalog`
  (e.g. `owner.notify`) classifying the trusted-origin carve-outs.
- [x] Add a per-action `token_requiring` flag so `gate()` knows which actions must
  validate a selection token.
- [x] Enumerate all eight effect paths (see design.md table) as classified catalog
  entries: `gated-shell` / `post-gate-approved-effect` / `kernel-origin-gated` /
  `internal-maintenance-non-effect`.

## 3. Kernel wiring

- [x] Route `notify_owner_best_effort` (`pipeline/mod.rs:147–157`) through
  `gate()` with `ActionOrigin::Kernel` and the catalog-registered `owner.notify`
  action; retain the `owner.notified` audit (mod.rs:150). Approval-exempt, audit-
  never-exempt.
- [x] In `dispatch_read_selected_thread` (`api/actions.rs:367–441`), remove the
  token validation block (actions.rs:384–421) now performed by pure `gate()`;
  keep the atomic single-use consume (actions.rs:413–416) at dispatch.

## 4. Digest re-derivation at approval-effect time

- [x] Shell-facing request DTOs MUST structurally exclude digest fields
  (`target_digest`/`payload_ref.digest`).
- [x] In the `create_approved_draft` effect handler (`pipeline/approval.rs:206–359`),
  re-derive the payload digest from artifact-store bytes (payload re-fetched at
  approval.rs:216) and deny on mismatch with the approved payload digest; target
  digest re-derivation already exists at `approval.rs:290`.
- [x] Deny the effect whenever the kernel-re-derived digest diverges from the
  approved digest.

## 5. Characterization tests — one per enumerated carve-out entry

- [x] `notify_owner_best_effort` (kernel-origin-gated): `gate()` auto-allows with
  `Kernel` origin and emits `owner.notified`; a kernel-origin call outside the set
  is denied.
- [x] `create_approved_draft` (post-gate-approved-effect): only reached after a
  `gate()` Allow; re-derived payload/target digest mismatch denies.
- [x] `activate_approved_artifact` (post-gate-approved-effect): only reached after
  a `gate()` Allow; bytes re-parsed from the kernel store.
- [x] `dispatch_read_selected_thread` (gated-shell, token-validated in `gate()`):
  `gate()` denies missing/expired/foreign/wrong-type token; dispatch performs the
  atomic consume after allow.
- [x] `dispatch_lyra_preview` / `propose_draft_creation` (gated-shell): gated;
  kernel computes `target_digest` (actions.rs:333).
- [x] `dispatch_artifact_propose` (gated-shell): gated; kernel computes
  `target_digest` (artifact_propose.rs:147).
- [x] `sweep_expired_grants` (internal-maintenance-non-effect): no external effect;
  kernel-origin audit path.
- [x] `answer_callback_query` (internal-maintenance-non-effect): control-plane ack,
  no security-relevant effect.

## 6. Threat-claims register rows (implementation task — NOT edited here)

- [x] Add/adjust rows in `docs/threat-claims.md` asserting: every effect path is an
  enumerated, tested carve-out; kernel-origin effects are audit-never-exempt;
  selection-token validation is a `gate()` property; digests are kernel-re-derived
  (no shell-supplied digest trusted). This is a docs register edit performed during
  implementation, not by this change directory.

## 7. Decision-log D-055 (implementation task — NOT edited here)

- [x] Add **D-055** to `.raw/openspine-decision-log.md` (index + changelog rows)
  capturing the four settled decisions: carve-out enumeration as catalog data;
  `KernelOrigin` marker (approval-exempt, audit-never-exempt routing); selection-
  token validation inside pure `gate()` with dispatch-side consumption; kernel-re-
  derived digests at approval-effect time. This change directory edits no
  decision-log file.

## 8. Docs

- [x] Update `gate-action-api`, `selected-thread-email-preview-slice`, and
  `digest-bound-draft-approval` capability references where they describe token
  validation or digest trust, to point at the new `gate()`-resident validation and
  kernel re-derivation. No user-facing terminology change beyond that.

## 9. Validation

- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`.
- [ ] `bash scripts/check-file-sizes.sh` — files ≤500 lines.
- [ ] `npx --no-install openspec validate harden-gate-trusted-paths --strict` and
  `./scripts/check.sh` green.
- [ ] Independent reviewer pass on the diff before commit (authority/spec-
  conformance lens).
