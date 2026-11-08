# Inactive package installation

Package installation retains a validated declarative assistant package. It does **not** select that package, activate an agent, grant permissions, connect an account, start a model, or change the active artifact registry.

The inactive installation slice implements specification [#273](https://github.com/George-RD/openspine/issues/273). Exact installed comparison implements [#281](https://github.com/George-RD/openspine/issues/281). Package selection is [`openspine use`](package-selection.md); installation itself never selects.

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

To assess a retained candidate against the actual configured base, use
[`package review`](package-review.md). It reports compatibility, authority-bearing
changes, learned-component consequences and outstanding work without approval
or selection.

## What a receipt means

Successful installation reports `installed-inactive`, `selected: false`, `active: false`, and `activation_supported: false`. `package list` later reports `selected: true` (`status: selected-at-next-start`) for the one installation [`openspine use`](package-selection.md) committed; `active` stays `false` because nothing is live-activated. Its durable receipt binds the package ID, positive integer revision, inventory format version, content digest, manifest digest, installation ID, provenance, timestamp, and audit identity/sequence.

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

## Operator flow and readiness

Use the same `--config` for every command, and the same OS account that owns the
instance. Local owner-only state and keys are the trust assumption; local source
provenance does not authenticate a publisher or prove human presence.

For a fresh native instance, use the existing bootstrap, not installation as an
initializer:

```sh
openspine --config ./openspine.yaml init --owner 987654321 --name Owner \
  --model hf.co/LiquidAI/LFM2.5-1.2B-Instruct-GGUF:Q4_K_M \
  --base-url http://127.0.0.1:11434
openspine --config ./openspine.yaml chat --once "Hello"
openspine --config ./openspine.yaml install lyra --json
openspine --config ./openspine.yaml setup --check
openspine --config ./openspine.yaml package review "$INSTALLATION_ID" --json
openspine --config ./openspine.yaml use "$INSTALLATION_ID"
openspine --config ./openspine.yaml setup --check
openspine --config ./openspine.yaml chat --once "Hello again"
```

Substitute your real owner ID and a model served by your endpoint. The bundled
package must be available for bootstrap. `init` accepts only the four shipped
seed workflows at their embedded digests; it does not accept arbitrary learned
overlays. Installation and setup never silently select a candidate. Take
`INSTALLATION_ID` from the receipt, review it, and type exactly
`select <installation-id>` at the `use` terminal prompt.

For an existing eligible deployment, keep its database, keys and owner binding,
stop its service and use the inspect/install/review/use flow above without
reinitializing it. An explicitly configured declared `lyra_dir` is adopted as the
legacy source on first selection. Both source and target must name the same
product. `use` cannot migrate another product into a populated instance.

The runtime, init and package maintenance share one exclusive lifetime lock.
Stop the service before install/review/use. Outstanding durable grants, approvals,
workflow steps and timers, dispatched workers, tasks, review requests,
notifications and unknown external-effect outcomes can still block an idle
process. Wait or resolve them through their normal lifecycle, never delete their
rows or clear fences. Lyra terminal task grants normally last 120 seconds,
email-drafter grants 180 seconds, kernel owner-review/reconfirmation grants
300 seconds and notification grants 60 seconds; custom package limits can
differ. A stopped/completed chat does not immediately expire its grant.

The **package snapshot check** acquires no lifetime lock and performs no schema
migration, installation recovery, owner creation, selection or reconfirmation.
It verifies database evidence in one existing read-only SQLite snapshot, copies
the selected identity and receipts, and closes that transaction before retained
object reads or typed validation. A busy database reports `database-busy`: retry,
not migration or corruption. No ledger rows, database contents or object bytes
are changed; SQLite may use its normal WAL/SHM coordination for WAL-mode ledgers.

The overall `setup --check` command also opens the credential vault. That path
acquires the lifetime lock when free; when the service owns it, the vault
degrades to `Locked` and readiness reporting continues without it. Thus the
command can run alongside the service, but is not unconditionally lock-free.
Its result is point-in-time diagnostics, not authorization to start or switch.
Fresh setup that declines seed consent may have a vault/data directory but no
kernel database: with no package namespace it remains a fresh legacy instance.
A package namespace without its database still fails closed.

The package checks distinguish:

- **legacy configured base**: exact declared product/revision/digests when
  inspectable, with `installation=none`; undeclared legacy layouts explicitly
  report that exact declared identity is unavailable;
- **installed-inactive candidate**: exact installation/product/revision/digests
  and original provenance, never implied to be selected or compatible;
- **committed selection**: the ledger generation and exact retained identity;
  `lyra_dir` is ignored, including a conflicting explicit value or removed source;
- **unavailable selected package**: missing/corrupt retained bytes,
  current-runtime incompatibility or invalid ledger replay; no legacy/bundled/
  earlier-object fallback, no candidate text in errors;
- **ledger-schema-unavailable**: readiness cannot migrate or verify an older
  schema; stop the service and use normal runtime startup with the matching
  runtime, then check again. Preserve damaged state rather than resetting it.

Returning to an earlier installed receipt uses
`openspine --config ./openspine.yaml use "$EARLIER_INSTALLATION_ID"`, followed
by normal runtime/chat startup. Generations increase for every selection,
including A → B → A. This is not a database restore and never an edit to
`lyra_dir`. Preserve overlays, credentials, owner identity, installation and
selection receipts, revoked/paused/erased state, consumed approvals and budget
history. Compatibility changes require the ordinary learned-artifact
reconfirmation and standing-rule review; returning to A cannot revive authority.
There is no hot reload.

This completes epic #117's local selection/readiness/recovery tranche, not
signed/version-pinned distribution, remote package fetching, cross-product data
migration or live Bell integration.
