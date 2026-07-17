# Design: minimal seed workflow set

## Seed selection and ids (AD-153)

The four seeds are exactly the minimal set AD-153 names. Each ships under a
distinct `_seed` id so it never collides with the production base fixtures
(`owner_control_conversation`, `selected_thread_email_reply_draft`) already
loaded from `artifacts/lyra/workflows`. A collision would trigger the
base/overlay identity-collision exclusion path in `overlay_startup::load`; the
`_seed` ids avoid that and let the seeds live purely in the overlay namespace.

| Seed id | Shape |
| --- | --- |
| `owner_control_conversation_seed` | received → composed → replied (no approval) |
| `email_draft_with_approval_seed` | selected → drafted → awaiting_approval(approval) → approved |
| `research_and_brief_seed` | launched → researched → briefed (owner-directed, no approval) |
| `customer_service_intake_seed` | received → classified(escalation) → drafted → awaiting_approval(approval) → approved |

## Declarative format (D-087..D-090)

Every seed is a `WorkflowManifest` reusing the already-merged substrate:
- `initial_state` plus uniquely-identified `states` and directed `transitions`
  with exact ids; `validate()` enforces reference integrity and the
  approval-state rules (an approval-required state must declare an
  `approval_action`; a transition may not leave and enter approval states in one
  step).
- Typed steps (`agentic`/`deterministic`, `reasoning_tier`) per D-089's tier
  resolution hook.
- The email-draft and customer-service seeds mark a state `approval: required`
  with `approval_action: email.create_draft`. `WorkflowStateMachine` binds the
  manifest digest at run start (D-090) and, on leaving an approval-required
  state, requires the Store-backed `ActionRequest` + `ApprovalRecord` matching
  the exact action and immutable payload/target digests (D-087/D-088).

## Overlay shipping, not kernel fixtures (AD-070/AD-071/AD-080)

Seeds live in `artifacts/overlay-seeds/workflows/*.yaml` at the repo root and
are embedded via `include_str!` into `crate::seed_workflows`. On first boot,
`overlay_startup::load` calls `seed_workflows::materialize_missing`, which writes
any absent seed into `<data_dir>/artifacts.d/workflows/` using the canonical
`artifact_loader::overlay_filename`. The existing overlay loader then discovers
them; because they have no prior provenance row, `overlay_startup` records each
as a `LegacyMigration` learned artifact in the `Overlay` namespace
(`ReconfirmationRequired`), exactly like any other artifact discovered on disk
without provenance. This reuses the shipped, security-reviewed quarantine path
rather than inventing a new "auto-active" provenance branch.

Idempotency and once-per-fresh-install: `materialize_missing` records a
persisted `kv_state` marker (`seed_workflows_materialized`) and short-circuits
once it is set, so the kernel seeds exactly once per fresh install. A seed the
owner deletes after first boot is NOT re-created (the marker wins over the
shipped file), and an owner-upgraded higher version stays live
(highest-version-wins). The file-level `target.exists()` check adds a second
guard: an existing overlay file is never overwritten, so an owner's edits to a
present seed survive. A crash between the file writes and the marker write is
safe — the files exist (0 rewrites on rerun) and the marker is set once
materialization completes.

## Out of scope

Production workflow driving/threading is deferred to `worker-runtime` (D-090);
this change ships the tested artifacts and the boot materialization only. The
`customer_service_intake_seed` `required_agent`/`required_capability_pack`
forward-declare a dedicated CS agent/pack that is itself a future miner proposal;
the workflow definition is valid substrate regardless.

## Security

No new persisted schema, migration, or change to the kernel `append_audit` /
provenance boundary. Materialization performs no model calls, no secret reads,
and no authority grants. Digest-bound approval behavior is entirely inherited
from the already-reviewed state-machine substrate. Required capability/test
claims: none added (no new security guarantee beyond what D-087..D-090 already
register).
