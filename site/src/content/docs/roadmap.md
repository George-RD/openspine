---
title: Roadmap
description: What has shipped, what was backfilled, and what's deliberately deferred.
---

This page shows what we have built so far and what we plan to build next.

Phase-honest, no dates — see
[`openspec/openspine-change-sequence.md`](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md)
for the underlying OpenSpec change sequence and
[`docs/threat-claims.md`](https://github.com/George-RD/openspine/blob/main/docs/threat-claims.md)
for what's actually proven, not just planned.

## Shipped

- **Core runtime schemas** — versioned, `deny_unknown_fields` object kinds
  for every runtime concept, plus canonical-JSON digests.
- **Authority composition** — deny-by-default, deterministic intersection
  of route/workflow/agent/pack/policy into a task grant.
- **Gate action API** — the single `gate()` mediation boundary every
  effectful action passes through; grant budgets (`max_model_calls`,
  `max_artifacts`) enforced at runtime, not just advertised.
- **Telegram owner-control channel** — the first verified owner-control
  channel; structurally verified sender identity, never trust-by-identity.
- **Selected-thread email preview** — `/draft <thread_id>`, a live Gmail
  fetch, and a model-drafted reply previewed over Telegram, with no draft
  creation and no send.
- **Digest-bound draft approval** — a Gmail draft is created only after
  the owner approves the *exact* reviewed payload and target; a truncated
  Telegram preview refuses the approval button entirely (WYSIWYS) rather
  than risking approval of unseen content.
- **Hardening pass** — task tokens hashed at rest, expired grants swept,
  kernel-originated owner notifications audited.
- **Artifact-lifecycle slice** — `artifact.propose` → owner approval →
  activation into the live registry and an on-disk overlay, for routes,
  agents, workflows, capability packs, and policies. Every proposal
  requires the same explicit approval; prompt templates are not
  proposable at runtime.
- **Threat-claims register** — every security claim mapped to a real test
  (or an honest `manual:` justification), gated deterministically on every
  change.

## Backfilled

Implemented inside the slices above, with a capability spec added
afterwards once the shipped behaviour was directly inspectable:

- **Model gateway** — mediates every model-provider call that touches
  private context; the shell never sees a provider API key.
- **Audit and artifact store** — the append-only, hash-chained audit log
  and the encrypted, content-addressed artifact store.
- **Shell containment** — the `SandboxDriver` boundary (`ProcessDriver`
  dev-only, `DockerDriver` for real network isolation).

## Deferred, on purpose

Not gaps nobody noticed — decisions recorded in the decision log:

- **Secret intake** — bootstrap secrets (bot token, artifact key, provider
  keys) currently live in environment variables; a richer secret-management
  story is future work.
- **A real Gmail thread picker** — today's `/draft <thread_id>` requires a
  thread id copied from Gmail's own web UI; "browse recent threads" /
  "the one from Alex about the invoice" is explicit future scope, not a
  shortcut taken here.
- **Per-kind activation policies** — the artifact-lifecycle slice uses one
  canonical `artifact.activate` action id with uniform approval for every
  kind; the PRD's per-kind ids (`route.activate`, `workflow.activate`, and
  so on) remain candidate, unwired entries for a future change that
  deliberately wants different approval policies per kind.
- **Widening-detection heuristics** — every artifact proposal requires the
  same explicit owner approval today; a heuristic that lets a
  "safe-looking" proposal skip that button is itself an authority decision
  nobody has designed yet.
- **Quarantine and retirement runtime paths** — the artifact lifecycle
  schema already models these transitions; there is no runtime path to
  trigger them yet.
