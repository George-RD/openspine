# Productize Lyra as the default agent package

## Why

The repository already contains a working set of Lyra artifacts, but the package
boundary is implicit. The website names Lyra while the source tree presents
`main_assistant_agent` and related files without a single declaration explaining
that they form the default agent.

This creates two risks:

1. Product copy can get ahead of runtime truth.
2. A future installer may collapse personality, memory, and authority into an
   unsafe mutable bundle rather than preserving OpenSpine's existing separation.

## Change

- Add a declarative `artifacts/lyra/package.yaml` naming Lyra, its entry agent,
  included artifact families, memory contract, and security invariants.
- Add a human-readable personality contract that carries no authority.
- Name Lyra explicitly in the entry agent's purpose.
- Document the current package-loading behavior and the intended
  `openspine install lyra` interface.

## Non-goals

- No native package store or resolver in this change.
- No new authority path.
- No broadening of memory access.
- No connector credential changes.
- No claim that `PERSONA.md` is directly loaded by the kernel.

## Follow-up

Implement a transactional, versioned, auditable package installer with an
explicit selected-package field replacing the legacy `lyra_dir` name.
