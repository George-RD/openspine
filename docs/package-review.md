# Review an installed package transition

`package review` compares the configured base with one exact retained installation
and reports runtime changes, learned-component consequences and durable work.
It records no approval and selects no package. The review implements the read-only
slice [#285](https://github.com/George-RD/openspine/issues/285) of
[#284](https://github.com/George-RD/openspine/issues/284).

Stop the runtime first. Use an existing instance and an exact `installation_id`
from an installation receipt or `package list`:

```sh
openspine --config ./openspine.yaml package review "$INSTALLATION_ID"
openspine --config ./openspine.yaml package review "$INSTALLATION_ID" --json
```

Review uses the ordinary configuration, adjacent environment file, exclusive
data-directory lifetime lock and validated existing ledger. It never creates
an owner binding or replacement ledger. It does not start models, connectors,
timers, network ingress or an asynchronous runtime.

## Reading the result

The source is identified as `legacy-configured`: its exact bytes came from
`lyra_dir`. A matching installed receipt does not make that source a previously
approved installation. The candidate keeps its original installation receipt
and provenance. Review verifies retained bytes and validates the same captured
bytes with the current typed loader; it does not reopen the original candidate
directory. That original directory can already have been removed.

The versioned report contains exact endpoint identities, runtime-input digests,
base compatibility epochs, artifact changes and complete bounded typed details,
overlay consequences, durable-work counts and a canonical `plan_digest`.
Package-authored values are untrusted data. Terminal control characters are
escaped and package-supplied labels are distinguished from kernel findings.
Missing catalog descriptions or oversized required detail block review
completeness; omitted detail must not be treated as approved.
Owner descriptions come from the existing action or delegation catalog. An
action does not need a model-facing tool schema to have complete owner metadata.
`missing-owner-action-description` identifies genuinely missing descriptions;
the accepting slice must resolve these before treating a review as complete.

`approval: not-performed` and `activation_supported: false` always apply.
`semantic_review_complete`, `blockers` and `outstanding_work.quiescent`
describe the reported prerequisites. They are not approval, a promise of unchanged
permissions or importable authorization.
A document-only change carries no implicit authority conclusion.

`overlay_semantics` includes the complete bounded typed fields of admitted learned
components and their before/after registry visibility. Visibility does not mean a
standing rule has live permission to execute. Its pause, revocation, evaluation
and budget controls belong to the separate authority lifecycle.

Golden-set files found in the overlay directory are unversioned fixtures.
Startup does not merge them into its effective registry. Review retains their
source digests and bounded typed details, reports `version: null` and an
`ignored_fixtures` disposition of `not_loaded_by_runtime`, and never treats them
as activated or reconfirmable artifacts. A matching base fixture stays in use.
Proposal or learned-artifact controls attached to a golden set remain invalid.

When a base epoch changes, `required_transition_consequences` requires the future
selection transaction to invalidate reusable authority and require ordinary
reconfirmation before reuse. This is a consequence of accepting the transition,
not a demand to reconfirm against the old base first. Review performs neither
operation. Its captured overlay fingerprint does not bind all live authority
controls; acceptance must capture and check those additional controls itself.

Exit status zero means the assessment was produced, including its blockers.
Inspect the completeness, blockers, overlay and outstanding-work fields rather
than using exit status as permission to transition. Failure to capture trustworthy
inputs produces a nonzero exit status and a fixed, bounded diagnostic.

## Blockers and recovery

| Finding | Meaning |
| --- | --- |
| `installation-id-invalid` | The selector is not an exact canonical installation ID; configuration was not opened. |
| `installation-not-found` | No committed receipt has that installation ID. |
| `object-corrupt` | Retained bytes are missing or fail the recorded inventory identity. |
| `candidate-incompatible` | Retained integrity passed, but today's typed loader rejects the captured candidate. |
| `candidate-staging-unavailable` | Retained integrity passed, but private temporary loader staging failed. This is not corruption. |
| `current-base-unavailable` | The configured legacy base could not be captured and validated. |
| `package-id-mismatch` | The candidate belongs to a different product; this command does not migrate product data. |
| `overlay-state-unavailable` | Existing learned state could not be assessed reliably. |
| `outstanding-work-unavailable` | The durable-work census could not be completed. |

Live grants, unfinished workflows, timers, queued workers, unconsumed approvals,
unresolved effects and unknown persisted states prevent a quiescent result.
Terminal history stays visible without itself blocking quiescence. Use normal
completion, cancellation or reconciliation paths; review never clears work,
consumes approvals or releases an unknown-effect fence.

Excluded or reconfirmation-required learned components retain their files and
history. Review does not grant reconfirmation, materialize seeds, repair missing
committed overlay files or recover interrupted installations. It also leaves
pending key erasure/publication evidence untouched.

`package compare` remains a separate retained-integrity operation. It does not
need loader staging and does not establish current compatibility. If temporary
storage is unavailable, comparison can still succeed while review cannot finish.

## What remains unchanged

Review preserves configuration, key payloads, package receipts, learned/control
state, approvals, audit rows and overlay files. The ordinary maintenance opener
can perform existing schema migrations, create package-storage directories and
clean controller temporary files. SQLite may update its journal or checkpoint
files. These opening effects are separate from review; this is not a guarantee
that every filesystem byte or metadata timestamp remains untouched.

Identical captured inputs and work classifications produce the same plan digest.
Expiry is assessed once per command; a grant or approval expiring between two
commands can legitimately change the work classification and plan. No review
timestamp or generated review ID is added to the deterministic report.

Package acceptance, managed startup and rollback are later delivery slices.
This command does not add `use`, a selected pointer or a permission bypass.
