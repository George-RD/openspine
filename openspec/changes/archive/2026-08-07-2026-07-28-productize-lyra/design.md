# Design

## Package boundary

`artifacts/lyra/package.yaml` is the package declaration. It names the persistent
entry agent and records the package's intended artifact families, memory contract,
and security invariants. The runtime continues to load the existing typed artifact
directories; the package declaration is additive metadata for humans and the future
installer, not a new authority source.

## Nix-like mental model

The package describes desired state. Effective execution is derived rather than
mutated imperatively:

```text
package declaration
  + active routes
  + agent manifest
  + workflow manifest
  + capability packs
  + policies and caveats
  + approvals
  + runtime limits
  = bounded task grant
```

An install operation should place an immutable versioned package in a local store,
then atomically update a selected-package reference. Rollback should select the
prior package version; it should not reconstruct state by reversing mutations.

## Identity and memory

A single `soul.md` is not used as the runtime state container. The design separates:

- a human-readable identity contract (`PERSONA.md`);
- learnable persona overlay elements;
- typed memory artifacts with explicit classes, scopes, provenance, and denial;
- authority-bearing policies and capability packs.

This preserves the useful authoring ergonomics of a soul document without making
free text privileged or allowing personality edits to widen permissions.

## Compatibility

The existing `lyra_dir` configuration and artifact loader remain unchanged. A
future package-store change should provide a migration path and package-neutral
configuration while continuing to load existing deployments.
