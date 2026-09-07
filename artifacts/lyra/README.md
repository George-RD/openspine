# Lyra agent package

Lyra is the first named agent package shipped with OpenSpine. The package is a
declarative set of agent manifests, routes, workflows, capability packs,
policies, and templates loaded by the kernel from this directory.

## Mental model

```text
OpenSpine kernel
  composes and enforces authority
          │
          ▼
Lyra package
  declares intended behavior and workflows
          │
          ▼
Task grant
  effective authority for one execution
```

Lyra is not a privileged process and does not carry raw connector credentials.
Her manifests describe intended tools. Effective permissions are resolved by
the kernel from route, agent, workflow, capability-pack, policy, caveat, and
approval constraints. Explicit deny wins.

## Package contents

- `package.yaml` — package identity, entry agent, memory contract, and invariants.
- `PERSONA.md` — human-readable identity contract; not an authority source.
- `agents/` — the persistent owner-facing coordinator and bounded workers.
- `routes/` — event-to-agent routing declarations.
- `workflows/` — typed workflow state machines.
- `packs/` — capability declarations available for composition.
- `policies/` — global and workflow policy constraints.
- `templates/` — prompt templates used after authority is resolved.
- `golden_sets/` — reference cases used to evaluate a proposed model swap.

## Current installation

The repository configuration already defaults `lyra_dir` to `artifacts/lyra`, so
a source checkout starts with this package as its base registry.

For an external deployment, set the same directory explicitly:

```yaml
lyra_dir: /opt/openspine/packages/lyra
```

Inspect that directory without starting the runtime:

```sh
openspine package inspect artifacts/lyra --json
```

Inspection validates the declaration and exact captured bytes. It does not
install or select a package, and does not authenticate a publisher. Inactive
installation and audit receipts are tracked separately in #275; activation is
not part of that installation step. See [inspection](../../docs/packages.md).

## Memory and personality

Lyra does not use a single mutable transcript or privileged `soul.md`.

- Personality is seeded as learnable persona overlay artifacts.
- Durable memory is typed and scoped.
- The main assistant manifest permits selected owner preferences and workflow
  state while denying sensitive classes such as raw email bodies, secrets,
  health, finance, and private family information.
- Corrections can update behavioral overlays without changing authority.
