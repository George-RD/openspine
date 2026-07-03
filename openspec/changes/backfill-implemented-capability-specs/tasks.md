# Tasks: Backfill implemented capability specs

## 1. New capability specs

- [x] `model-gateway`: private-context calls constructed kernel-side,
      credentials never reach the shell, untrusted-context wrapping with
      a per-call randomised delimiter, templates from the kernel
      registry only, conversation state stores only role + digest.
- [x] `audit-artifact-store`: append-only hash-chained audit log,
      startup chain verification (not a CLI — corrected from the initial
      assumption, see design.md), AES-256-GCM encrypted content-addressed
      artifact blobs, digest re-verification on read, task tokens hashed
      at rest.
- [x] `shell-containment`: shell environment allow-list, Docker
      containment flags (no-public-egress network, read-only rootfs,
      non-root user), Process-driver external-communication refusal
      without explicit opt-in, documented plaintext-transport trust
      assumption.

## 2. Restore dropped dev-process requirements

- [x] Diff the archived `define-openspine-development-process` spec
      delta against the current canonical spec.
- [x] Restore the "`tasks.md` grants no runtime access" scenario under
      "OpenSpec artifacts MUST NOT be treated as live runtime authority."
- [x] Restore the "archive MUST preserve rationale" bullet list under
      "Completed OpenSpec changes MUST be archived."

## 3. Close the loophole going forward

- [x] Add an ADDED requirement on `openspine-development-process`:
      security-load-bearing subsystems must gain a capability spec in
      the change that implements them.

## 4. Docs

- [x] `openspec/openspine-change-sequence.md`: move
      `implement-model-gateway`, `implement-audit-artifact-store`,
      `implement-shell-containment` into a new "Backfilled" subsection
      under Completed.
- [x] `openspec/openspine-change-backlog.md`: checked for the same three
      ids — none present, no edit needed.
- [x] `docs/kernel-http-contract.md`: added a "Transport trust
      assumption" section documenting the plaintext-HTTP-over-internal-
      network decision, cited by the `shell-containment` spec.

## 5. Decision log

- [x] D-049 appended to `.raw/openspine-decision-log.md`.

## 6. Validation

- [x] `npx --no-install openspec validate --all --strict` (10
      capabilities).
- [x] `./scripts/check.sh backfill-implemented-capability-specs`.
