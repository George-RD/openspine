# Proposal: Worker runtime commissioning and reply chokepoint

## Dependencies
- `openspine-schemas` worker, grant, grant-chain, and briefcase types
- `openspine-authority` caveat-chain sub-grant minting (`mint_worker_grant`)
- Existing grant binding, identity resolution, registry, audit/event-bus, and Store
- Canon AD-030, AD-033, AD-035, AD-101; decision log D-083, D-085, D-086, D-012, D-073, D-075

## Problem
A master agent can perform multi-step work, but it has no structurally safe way to
delegate a *narrower* slice of its own authority to a sandboxed worker and recover the
worker's output. Without a caveat-only attenuation path, a worker either inherits the
master's full authority (widening) or the kernel has no honest place to draw the
"reply" boundary — so worker egress and master egress blur into one undifferentiated
channel.

## Proposed Solution
A master agent commissions a worker by minting a **caveat-chain sub-grant** of its own
grant (Macaroons-style): the child's MAC extends the parent's sealed tip, so the child
verifies offline against only the HMAC key and its embedded chain — no parent or
root DB lookup (the change's leaning interpretation of AD-101). Every worker grant is
locked to an empty `OutputChannelAllowlist`, so its *effective* output channels are
provably empty (AD-035 reply chokepoint): direct egress is prevented by authenticated
attenuation enforced at the gate, not by convention. The worker's only outbound path
is `worker.report_result`, which the kernel records as a `worker.result` **bus event**
on the worker grant's aggregate; the master consumes it through the ordinary event bus
and relays it through its own separately-gated reply path. A commissioned worker receives a
**briefcase** packed for its own grant (D-085), never the shared task board. Recording is
receipt-keyed and fail-closed (D-083): a result for an already-terminal dispatch is rejected,
never replayed.

## Acceptance Criteria
- A worker sub-grant is a caveat-chain attenuation of the parent; it verifies offline
  (no store/network) and can never exceed the parent's authority.
- A child grant minted with a widened action or expiry is rejected.
- A worker result surfaces as a consumed `worker.result` bus event carrying the
  structured payload; it is replay-protected.
- A worker grant has no effective output channel; direct worker egress is impossible.
- A commissioned worker receives a briefcase scoped to its own grant, not the board.
- `cargo test`, `cargo clippy -D warnings`, `cargo fmt --check`, file-size, claims,
  and `openspec validate implement-worker-runtime --strict` all pass.

## Out of Scope
- Mining/learning briefcase depth or visibility tables (substrate only).
- The worker HTTP protocol / sandbox spawn mechanics beyond recording the grant.
- End-to-end spawn-and-collect integration against a live sandbox (covered by the
  bus-event and store-level acceptance tests here).
- New capability-pack or routing changes; this change only attenuates existing
  master authority.
