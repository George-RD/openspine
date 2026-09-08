# Inspect and install the Lyra source package

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

## Inactive installation (Linux and macOS)

Use an existing instance initialized through `openspine init` or `openspine setup`.
Stop its runtime before package maintenance. The commands use its normal
configuration, adjacent owner environment file, existing ledger and exclusive
data-directory lock; the global `--config` option applies.

```sh
openspine install lyra --json
# Or retain an explicit local package:
openspine install --from /path/to/lyra --json
openspine package list --json
openspine package receipts --json
```

Success means `installed-inactive`: the exact validated bytes and an audit-bound
receipt are retained. It does not change `lyra_dir`, select or activate the
package, connect accounts, or grant permissions. See
[inactive installation](../../docs/package-installation.md) for retries,
recovery and integrity failures.

This bundle is revision 2 because these instructions changed bytes included in
the content inventory. Previously installed revisions and receipts are not
rewritten. Future changes to any bundled file, including documentation, need a
new package revision before installation alongside an existing revision.

Package selection (`openspine use lyra`), activation and signed distribution
remain separate, unshipped work.
