# Inactive package installation

Package installation retains a validated declarative assistant package. It does **not** select that package, activate an agent, grant permissions, connect an account, start a model, or change the active artifact registry.

The inactive installation slice implements specification [#273](https://github.com/George-RD/openspine/issues/273). Exact installed comparison implements [#281](https://github.com/George-RD/openspine/issues/281). Package selection and activation remain separate work.

## Commands

Use an existing instance created with the native `init` or `setup` flow. Stop its runtime before package maintenance: these commands use the same exclusive data-directory lifetime lock. They read the normal configuration and adjacent owner environment file and open the existing `data/kernel.db`; they do not create replacement keys or an alternate ledger.

```sh
# Inspection remains offline and does not require owner configuration or keys.
openspine package inspect ./my-package --json

# Retain either the runtime's bundled Lyra package or an explicit local package.
openspine install lyra --json
openspine install --from ./my-package --json

# Inspect the durable installed index and content-free operation receipts.
openspine package list --json
openspine package receipts --json

# Use the exact installation_id values from two committed receipts.
openspine package compare "$FROM_INSTALLATION_ID" "$TO_INSTALLATION_ID" --json
```

The normal global `--config` option applies to installation, listing and comparison. No remote package registry or URL fetching is supported. Native package maintenance is implemented for Linux and macOS; other platforms refuse rather than substitute weaker filesystem semantics.

## What a receipt means

Successful installation reports `installed-inactive`, `selected: false`, `active: false`, and `activation_supported: false`. Its durable receipt binds the package ID, positive integer revision, inventory format version, content digest, manifest digest, installation ID, provenance, timestamp, and audit identity/sequence.

`local-unverified` means an explicit local source. `runtime-bundled` records selection of the bundled-package source. Neither is publisher authentication, a signature-verification claim, an approval, or authority to perform actions. An exact retry preserves the original receipt and provenance; it cannot upgrade a local installation's trust label.

The digest covers the sorted inventory of exact captured file bytes, including `package.yaml`. Inspection captures and validates those bytes once. The installer consumes that retained snapshot, not a second read of the candidate directory.

## Compare two installed revisions

`package compare` takes two exact, canonical installation IDs, not package names, revision aliases, content digests, `latest`, or source paths. It compares the retained objects referenced by their original committed receipts. Deleting or changing the original source directories does not change the comparison. Either endpoint may be older; neither is assumed to be the active package.

The versioned report includes both original receipts and provenance, `identical`, counts of added, removed, modified and unchanged files, and changes sorted by normalized path. Each changed file carries its path, category, and before/after byte lengths and digests. An absent side is `null`. Renames are a removal and an addition; package document text is never printed. Without `--json`, the same fields are pretty-printed.

Categories are deliberately conservative: `package.yaml` is `package-declaration`; YAML/YML in supported artifact families is `runtime-input`; other accepted files are `documentation`. `runtime_inputs_changed` and `package_declaration_changed` indicate byte changes, not their meaning. Even a formatting-only policy edit is a runtime-input change. Neither flag being false is an approval.

A successful report states `basis: retained-byte-integrity`, `runtime_compatibility: not-evaluated`, `authority_review: not-performed`, `publisher_verification: not-performed`, `activation_supported: false`, and `active_package_identified: false`. Comparison does not evaluate current runtime schemas, overlay compatibility, permission widening or narrowing, or publisher trust. It cannot approve, select or activate a package.

Both retained objects must pass full integrity checks before a report is produced, including when comparing an installation with itself. Invalid IDs return `installation-id-invalid`; unknown committed IDs return `installation-not-found`; missing or changed objects return `object-corrupt`. Errors contain no partial diff or source document text. Objects are not repaired or reinstalled. Comparison does not need temporary loader staging.

Comparison does not append installation, audit, approval or authority rows, change active files/configuration/keys, or recover interrupted package installations. It still uses the ordinary maintenance opener, which may perform normal schema migrations, create storage directories and clean controller temporary files. It is not a promise that every filesystem byte remains untouched.

## Storage and retries

Objects live under the canonical data root at `packages/objects/<content-digest-hex>`, not under the configured active package or `artifacts.d`. The installer refuses overlaps with active artifact paths, destination links, and shared-writable storage directories.

Publication uses an owner-only staging directory on the same filesystem. Files and directories are synchronized before atomic no-overwrite publication. The installed-index row and successful audit event are then committed in one Store transaction. Directory presence alone never means installed.

An exact ID/revision/digest retry verifies the retained object and returns the original receipt with `idempotent_retry: true`. Different bytes under an existing ID and revision produce `revision-conflict`; they never overwrite the installed object. Different revisions may coexist, still unselected and inactive. Changes to any bundled file, including documentation, require a new package revision to avoid this conflict. The corrected Lyra installation instructions ship as revision 2; existing revision-1 objects and receipts are unchanged.

A process interrupted before the index/audit transaction commits leaves no successful installation. Recovery records the interrupted operation, removes only staging belonging to known attempts, and preserves published objects. A later matching retry may reuse an orphan only after validating the candidate, verifying the retained object's exact identity, and synchronizing it. An interruption after commit preserves the same installation identity and successful receipt.

If post-commit staging cleanup fails, the successful result sets `cleanup_pending: true`. The installation remains committed; later maintenance retries cleanup. Unknown staging entries are not swept, and published objects and audit history are never garbage-collected by these commands.

## Missing or corrupt state

Listing reads the durable index and verifies each retained object's full inventory against its recorded package ID, revision, inventory format, content digest and manifest digest. Missing, modified, appended, linked, or filesystem-invalid payloads are reported as `unavailable-or-corrupt`. The receipt remains visible, but an exact installation retry refuses to repair or replace the object silently.

Retained-object verification does not create a temporary loader tree. An unavailable temporary directory therefore does not by itself mark intact retained bytes as corrupt. `available` means the recorded bytes are present and match their identity; it is not a fresh typed-artifact validation, current-runtime compatibility check, publisher verification or activation approval. Inspecting a new candidate still performs full typed loader validation and requires private temporary storage.

An index/audit mismatch or broken audit chain refuses package maintenance. Missing, empty, and unrelated database files are not initialized by a maintenance command. A pending export/restore is also refused: finish the normal runtime recovery procedure first, rather than applying it through a package-management shortcut.

Package maintenance does not invoke the runtime standing-rule sweep. Normal runtime startup retains that safety behavior. Existing authority records, active-package configuration, credentials, and owner keys are outside these commands' mutation scope.
