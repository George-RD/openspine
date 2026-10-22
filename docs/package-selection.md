# Select a package: `openspine use`

`openspine use <installation-id>` makes one exact, previously installed package the
instance's managed base. It implements specification
[#284](https://github.com/George-RD/openspine/issues/284) through
[ANO-822](https://linear.app/another-workspace-for-me/issue/ANO-822) (formerly
[#286](https://github.com/George-RD/openspine/issues/286)). It is a local operator command: it
neither starts the assistant nor reloads a running process, and no worker tool, HTTP
endpoint, package hook, model suggestion or `--yes` flag can select a base.

Stop the runtime first. `use` takes the same exclusive data-directory lifetime lock as
the other package commands, and it needs an existing instance created by `init` or
`setup` (it never creates or rebinds an owner, keys or a ledger).

```sh
openspine --config ./openspine.yaml install --from ./my-package --json
openspine --config ./openspine.yaml package review "$INSTALLATION_ID"   # optional preview
openspine --config ./openspine.yaml use "$INSTALLATION_ID"
```

## What the command does

1. **Refuses early.** The installation ID must be canonical, and both stdin and stdout
   must be terminals. Before the environment file is loaded or any maintenance state is
   opened, the local state must be what this OS account controls: the configuration,
   `data_dir` and `kernel.db` must be owned by the current account, not links, and not
   writable by group/other; the key file must also be unreadable by others. Nothing is
   repaired, created or chmod'ed to make that true.
2. **Verifies the persisted owner.** The owner principal and identity come from the
   existing ledger and must match the configured owner. Package, JSON and CLI input
   never supply them, and nothing is bootstrapped.
3. **Reviews the exact transition.** It builds the same plan as
   [`package review`](package-review.md) and prints it. Any blocker (including outstanding
   durable work, a missing owner description, an incompatible candidate or an
   unrenderable detail) ends the command with no prompt. Blockers cannot be overridden.
4. **Presents one decision.** A package-specific, channel-neutral decision is stored with
   a paired audit receipt before the prompt. It binds the owner/instance MAC, source and
   target identities, plan digest, selection generation, runtime compatibility identity,
   the overlay/provenance fingerprint, a fingerprint of all live control state, the
   required reconfirmation consequences and a ten-minute expiry (never more than 30).
5. **Asks for consent.** You must type exactly `select <installation-id>` and press Enter.
   End-of-file, any other text, an oversized line or an expired window selects nothing.
   The terminal is a consent transport, not proof of identity or human presence.
6. **Rechecks and commits.** Under the same lock it captures everything again and
   compares every binding. A changed package object, source base, owner, runtime,
   overlay, erasure/pause/revocation/budget/consumption/fence row, outstanding work or
   generation makes the review stale and selects nothing. An unrelated audit append never
   stales a review. One Store transaction then compares the expected generation,
   consumes the exact decision, writes the selection, applies the authority consequences
   and appends the receipt: all of it commits or none of it does.

## Managed mode and startup

The first selection adopts the base you configured as `lyra_dir`, which must be a
successfully captured, declared base of the same product as the target. The database,
keys, owner identity, overlays and learned history are kept. A different package ID in
a populated instance is refused; it needs a separate product/data migration.

Once any selection history exists, managed mode is sticky. Startup resolves the
ledger-validated selection before any ingress, model, worker or timer starts, verifies
the retained bytes against their recorded identity, validates them with today's typed
loader and loads the registry from that captured copy. `lyra_dir` is no longer read and
may disappear; a configured non-default value is logged as ignored and never overrides
the ledger. A corrupt, missing, linked, tampered or now-incompatible selected object, or
a tampered selection ledger, stops startup. It never falls back to `lyra_dir`, the
bundled package, an earlier revision or an unindexed object.

`package list` marks the selected installation (`selected: true`,
`status: selected-at-next-start`); nothing is live-activated. `package review` and `use`
treat a managed selection as the current source (`source.mode: managed`) without opening
`lyra_dir`. A candidate that is already selected is a blocker.

## Rollback is `use` of an earlier revision

Rolling back selects an older installed revision through the same plan, consent,
quiescence, compatibility and atomic-commit path. It is a new, higher generation, never
a database restore: A to B to A is generations 1, 2, 3, and an approval presented for
A to B can never be consumed after A to C to A. An earlier revision the current runtime
rejects is unavailable even if it once ran.

Revocations, pauses, erasure, consumed approvals, budget and quota history, installation
receipts and unknown-effect fences are not touched by a selection, forward or backward.

## Authority never inherits package consent

Accepting a transition approves the base transition only. When the base compatibility
epoch changes, the same transaction advances an `invalidation_generation` on the selection
record and sends every affected learned artifact (including standing rules) back to
`reconfirmation_required`; startup then excludes it until the ordinary lifecycle runs.
Returning to an earlier epoch is another change, never a revival. In managed mode startup
does not refresh an owner-accepted row's epoch: a row that does not bind the current base
returns to reconfirmation, and an evaluation verdict that bound no compatibility digest is
stale for standing-rule activation. In the same transaction every active standing rule moves to `needs_review` and its open exceptions go stale, with the rule IDs recorded in the audit payload. Consent here supplies none of that reconfirmation.

## Failure and refusal reference

| Message | Meaning |
| --- | --- |
| needs an interactive terminal | stdin or stdout is not a terminal; nothing was opened. |
| not owned and writable only by this account | The local-state preflight failed; nothing was opened or changed. |
| persisted owner could not be verified | No persisted owner, or it differs from the configured owner. |
| review has blockers | Resolve the listed blockers through normal paths, then review again. |
| Consent was not given | EOF, cancel or the wrong phrase. |
| changed after consent | The named bindings no longer match; review again. |
| consent window expired | Review again. |
| `selection-ledger-invalid`, `selected-object-corrupt`, `selected-package-incompatible` | A managed selection cannot be resolved; no other base is used. |

A crash before the transaction commits leaves the previous selection; a crash after it
leaves the new one. Recovery only finishes committed state and never invents approval or
overwrites a receipt.

## Rollout restrictions

- Linux and macOS only; other platforms refuse. Local administrator compromise is outside
  this boundary: there is no cryptographic human-presence claim.
- The instance must be stopped and idle. A first runtime start opens reconfirmation
  requests for the shipped seed workflows; they count as outstanding approvals and block
  `use` until resolved through the normal owner lifecycle.
- No live reload, remote package fetch, signed distribution or agent-authored selection.
