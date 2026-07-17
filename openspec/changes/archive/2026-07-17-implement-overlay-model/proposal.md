# Proposal: Implement overlay artifact compatibility

## Dependencies

- Existing artifact lifecycle and digest-bound approval (`artifact-lifecycle`, D-011).
- AD-023, AD-070, and AD-071 are settled canon.

## Problem/Context

The loader already reads shipped fixtures and user activations from separate base and overlay directories, but the distinction is implicit. Activated learned artifacts have no durable link to the exchange that produced them, and an upstream update can leave references dangling without a visible owner decision.

## Proposed Solution

Model the base/overlay namespace explicitly, persist mandatory source-event provenance for every learned artifact, and add a deterministic compatibility pass over overlay references. A dangling learned route or workflow is marked for re-confirmation and excluded from the effective registry until the owner reviews it. Re-confirmation remains digest-bound and uses an opaque pending-review identifier. Generalized pattern nomination is represented as an explicit, reviewed opt-in and never automatically changes an overlay artifact into a base artifact.

## Acceptance Criteria

- An update removing a referenced base artifact produces a pending re-confirmation record and excludes the learned artifact from the effective registry.
- Learned artifact activation cannot persist without a non-null source event provenance link.
- Provenance survives restart in the kernel store.
- Upstream nomination requires explicit opt-in and leaves the artifact in the overlay namespace until normal review completes.

## Out of Scope

- Automatic promotion or publication of user artifacts into upstream fixtures.
- Full semantic compatibility analysis beyond typed artifact references.
- Counterparty key erasure, export, or restore.
