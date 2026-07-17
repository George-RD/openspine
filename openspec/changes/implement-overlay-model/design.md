# Design: Base/overlay compatibility

## Authority-sensitive decisions

1. `ArtifactNamespace::Base` identifies shipped upstream artifacts; `Overlay` identifies user-owned learned artifacts. The loader's directory boundary is authoritative and overlay files survive base replacement.
2. `LearnedArtifact` requires both the producing `source_event_id` and the encrypted exchange `source_exchange_digest`. `task_grant_id` remains authorization metadata, not provenance. Activation records provenance before exposing the overlay file.
3. Compatibility is deterministic and fail-closed. The pass checks typed references from learned routes and workflows against the post-update merged registry. Dangling references create a pending review and are omitted from the effective registry; the YAML lifecycle remains `active` because `quarantined` is terminal.
4. Re-confirmation is a durable pending review keyed by an opaque ULID and the exact YAML digest. A later owner action may reinsert the unchanged reviewed bytes; changed bytes require a fresh proposal.
5. Upstream nomination requires an explicit `depersonalized: true` assertion and normal digest-bound review. Nomination status is recorded, but namespace remains `Overlay`; no automatic publication occurs.

## Lifecycle state machine

For each `(namespace, kind, id)` the effective lifecycle is:

```text
excluded --owner proposes/recreates--> pending
pending --owner accepts exact digest + no base collision--> owner-accepted (reviewed dangling refs allowed)
owner-accepted --restart/fixed-point reload--> owner-accepted
owner-accepted --loader exposes highest activated version--> active
active --base update breaks a typed reference--> pending
active --higher overlay version activates--> active (older version is audited as superseded)
pending --restart--> pending (same version/digest reuses its request)
pending --changed bytes or stale version--> excluded (fresh review is required)
```

Initial dangling activation enters `pending` without recording or announcing
`active`. A base/overlay identity collision also enters `pending`; an owner
tap cannot restore the overlay into the base identity and must rename/re-propose.
Missing-provenance files are quarantined into `pending` with synthesized
`LegacyMigration` provenance. Restart processing is idempotent: exact duplicate
`(kind, id, version)` files hard-error, and only the highest activated version
for an id is exposed while older versions receive `artifact.superseded` audit.
## Failure behavior

Missing provenance metadata for an overlay file is rejected from the effective registry and audited. Store writes use NOT NULL provenance columns. Compatibility and nomination failures are fail-closed and never silently preserve authority.
