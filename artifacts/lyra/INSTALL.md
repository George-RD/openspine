# Inspect and use the Lyra source package

A source checkout already selects this directory through the default `lyra_dir`.
For an existing external deployment, `lyra_dir` continues to select its base
artifact directory; the runtime startup path is unchanged.

Inspect the package without owner keys, configuration or a running kernel:

```sh
openspine package inspect artifacts/lyra
openspine package inspect /path/to/lyra --json
```

This checks the declaration against the exact captured artifacts and reports a
versioned inventory and content digest. It does not install, select, activate or
publisher-verify the package. Explicit local paths are `local-unverified`.
See [package inspection](../../docs/packages.md) for the accepted layout, limits
and JSON contract.

Native inactive installation and audit receipts are separate work (#275).
Selection and signed distribution need further review. The eventual
`install` / `use` flow is not a shipped interface.
