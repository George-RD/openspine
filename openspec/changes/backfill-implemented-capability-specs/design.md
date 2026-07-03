# Design: Backfill implemented capability specs

## Approach

Each of the three new specs is derived by reading the actual
implementation and citing the test that enforces each requirement, not by
re-deriving intended behaviour from the PRD alone. Where the PRD or an
earlier planning note assumed something that the shipped code does not
actually do, the spec follows the code, not the plan:

- The PRD-era assumption that audit-chain verification is exposed as a
  first-class `openspine audit verify` CLI subcommand does not hold —
  `main.rs` has no such subcommand. What is actually implemented is a
  startup-time check (`Store::verify_audit_chain`, called from `main`)
  that refuses to boot the kernel if the chain is broken. The
  `audit-artifact-store` spec records that behaviour instead.
- Everything else (hash-chain construction, genesis value, AES-256-GCM
  per-blob encryption, content-addressing by plaintext digest, digest
  re-verification on read, sandbox env-var allow-listing, Docker
  containment flags, the Process-driver containment refusal) matches the
  code exactly as found, cited against the specific test that proves it.

## Dev-process restoration

`git show` against the archived
`2026-07-02-define-openspine-development-process` change's spec delta
was diffed against the current canonical
`openspec/specs/openspine-development-process/spec.md` to find exactly
what was lost when the canonical spec was condensed. Two losses were
found (see proposal.md); both are restored verbatim as `MODIFIED
Requirements` in this change's delta, keeping their original scenario
text rather than rewriting it.

## Forward-looking requirement

The new ADDED requirement ("security-load-bearing subsystems MUST gain a
capability spec in the change that implements them") is deliberately
narrow — it does not retroactively require every future change to touch
every spec, only that a change implementing a *new* such subsystem must
ship its spec alongside it, closing exactly the gap this change repays.
