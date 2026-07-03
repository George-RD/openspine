# Design: Artifact lifecycle slice

## Approach

Reuse every piece of existing approval machinery rather than inventing a parallel one: `artifact.propose`'s shape mirrors `propose_draft_creation` almost exactly (budget → persist payload as an artifact-store blob → persist a pending `ActionRequest` → send an approval button), and activation reuses the same `handle_draft_approval_callback` entry point, branching on `request.action.as_str()` rather than adding a second callback handler. The only genuinely new mechanism is the `ArtifactRegistry` becoming shared-mutable (a lock instead of load-once-at-startup) and the `data/artifacts.d` overlay directory that lets an activated artifact survive a restart without being baked into the fixture tree.

## Key decisions

- **Registry locking**: `AppState.registry` is wrapped in a lock (`parking_lot::RwLock` if already a kernel dependency, else `std::sync::RwLock`). Every read site takes a read guard held only across the synchronous scan it needs — never across an `.await` (enforced by `clippy::await_holding_lock`, not just convention).
- **Overlay layout**: `data/artifacts.d/<kind-plural>/<artifact_id>-v<version>.yaml`, loaded at startup after the fixture tree via `load_registry_into`, and written via write-to-temp-then-rename for the same atomicity guarantee the artifact store already relies on.
- **Uniform approval, no widening heuristic**: every proposal — regardless of kind — requires the same explicit owner approval (D-048). A "this one looks safe, auto-activate it" heuristic is itself an authority decision this slice deliberately does not make.
- **One canonical activation id**: `artifact.activate`, not a per-kind id (D-048, mirrors D-034's `email.create_draft` precedent). The PRD's per-kind ids remain candidate/unwired.
- **Templates excluded**: prompt templates are not a proposable kind. A template governs the model's instruction surface, not just authority shape — letting chat propose one is a different, larger injection-escalation surface than this slice closes.
- **Digest binding**: the approval's `target_digest` binds `{kind, artifact_id, version}` (not just the YAML payload digest) — this catches a swap of *which* artifact activates even in the pathological case where two proposals' YAML bytes coincidentally hash the same target.
- **Crash safety**: overlay-file-write happens before registry-insert. A crash between the two leaves the overlay file present and the registry stale until the next restart, which reloads it — an accepted gap, not silently swallowed (documented in code).

## Alternatives considered

- **A dedicated `ActivationRecord` type instead of reusing `ApprovalRecord`**: rejected — `ApprovalRecord` already binds exactly what this slice needs (payload digest, target digest, approver identity, expiry); a parallel type would duplicate D-011's guarantees for no behavioural gain.
- **Widening-detection heuristics** (e.g. auto-activate a route that only narrows an existing one): rejected for this slice — see Goals/Non-goals in `proposal.md`; explicitly deferred, not silently dropped.
