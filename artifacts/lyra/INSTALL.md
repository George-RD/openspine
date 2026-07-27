# Install Lyra

A source checkout already selects this package through the default `lyra_dir`.
For external deployments, copy this immutable package directory to a versioned
location and point `lyra_dir` at that path.

The planned native interface is:

```text
openspine install lyra
openspine use lyra
```

The native installer is intentionally deferred until it can provide a versioned
package store, atomic selection, rollback, integrity verification, and audit
receipts. A simple recursive copy would not meet OpenSpine's authority and
provenance model.
