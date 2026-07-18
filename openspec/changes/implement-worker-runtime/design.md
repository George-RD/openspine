# Design: Worker runtime commissioning and reply chokepoint

## Caveat-chain sub-grant minting (offline-verifiable)

`openspine-authority::worker_grant::mint_worker_grant` derives a child grant from a
*parent* (the master agent's own grant) as a Macaroons-style caveat chain. The child's
MAC is computed by `grant_chain::seal_child_from_parent_tip`, which folds one new
`ChainStep` (carrying the attenuation caveats) onto the parent's already-sealed tip.
Verification (`TaskGrant::verify_mac`) therefore needs only the HMAC key and the
child's own embedded chain — never a parent or root DB lookup. This is the
change's leaning interpretation of AD-101's offline-verifiable-at-gate requirement.

The root-authority payload (`allowed_actions`, `approval_required_actions`,
`denied_actions`, `allowed_egress_classes`, `output_channels`, `limits`,
`expires_at`, `user`, `purpose`, `event_id`, `route_id`, `agent_id`, `workflow_id`,
`capability_pack_id`, `thread_id`) is authenticated as part of the sealed tip and MUST
stay byte-identical across every hop. `mint_worker_grant` clones the parent wholesale
and touches only instance-only and appended-hop fields (id, issued_at, task_token,
parent_grant_id, chain, caveat_mac), so a child can never choose a different purpose,
agent identity, or widen any root list.

### Narrowing caveats (never widening)
- `ActionAllowlist` — intersected with the parent's effective allowed actions. Always
  added, even when empty, so a worker commissioned with no actions is structurally
  locked out rather than silently inheriting the parent's full effective set.
- `BoundParameter` — AD-036 parameter locks; a later hop may add a name but never
  change an already-bound value.
- `ExpiresBefore` — the worker grant can never outlive the parent's effective expiry.
- `OutputChannelAllowlist { channels: vec![] }` — unconditionally added, so the
  worker's *effective* output channels are provably empty regardless of the root list
  (AD-035 reply chokepoint).

Minting fails closed when the parent chain does not verify, when a requested action is
not in the parent's effective authority, when the requested expiry exceeds the
parent's, or when a bound parameter contradicts an existing parent binding.

## Master: interpret / commission / relay

`worker.commission` (kernel `api/worker.rs`) runs only after the gate has authorized
the caller's own grant. It mints the narrowed worker sub-grant, packs the worker's
**briefcase** (`crate::briefcase::pack_for_task`, D-085 — the worker receives a
briefcase, never the board), and persists the grant + briefcase atomically with the
dispatch row and the `authority.granted` bus event. It then spawns the sandboxed
worker; a spawn failure is audited but does not suppress the already-commissioned
grant.

`worker.report_result` is the worker's ONLY outbound channel. It records the structured
result and flips the dispatch terminal — it never touches any egress path. The master
**relays** the worker's result onward through its own separately-gated reply path; the
worker itself has no such path.

## Worker result as a consumed bus event (AD-035 / D-073)

`store::worker_dispatch::record_worker_result` appends a `worker.result` audit event
on the worker grant's aggregate (`task_grant:<worker_id>`), carrying the structured
`WorkerResult` as JSON payload. The master consumes it via the ordinary event-bus
replay/consumer path — the same substrate any other aggregate uses — so worker output
is just another event, not a side channel. Free-text `notes_ref` is still only a digest
ref inside the result, preserving D-012 plaintext discipline even though the rest of
the result is JSON-inlined for the consumer.

## Receipt-keyed, fail-closed terminal flip (D-083)

`worker_dispatch` persists one row per commissioned worker: `dispatched` once the
grant + briefcase land atomically, `terminal` once a result is recorded. The terminal
flip and the receipt check (`worker_dispatch_state` inside the same `BEGIN IMMEDIATE`
transaction) share no TOCTOU window, so a result for an already-terminal dispatch is
rejected with `StoreError::WorkerResultAlreadyRecorded`, never replayed. This is the
D-083 honest-denial guarantee.

## Reply chokepoint: no output channels on worker grants (AD-035)

Because `mint_worker_grant` unconditionally appends `OutputChannelAllowlist { channels:
vec![] }`, `grant_chain::effectively_allows_output_channel` returns false for every
channel the root may carry. The worker grant schema carries no egress-capable field the
worker can set; direct egress is impossible by construction and verifiable offline.

## Briefcases to workers, not the board (D-085)

`worker.commission` calls `pack_for_task` for the commissioned worker's own grant and
persists that briefcase keyed by the worker grant id. The shared `task_board` is never
written by the commissioning path; a worker sees only its own scoped briefcase.

## Tests (acceptance)

- `offline_chain_verify_multi_level` (authority crate): a two-level worker chain
  verifies offline against the key only.
- `child_cannot_exceed_parent_authority` (authority crate): widened action / widened
  expiry are rejected.
- `direct_worker_egress_impossible` (authority crate): the worker's effective output
  channels are empty.
- `result_is_consumed_bus_event` + `worker_grant_verifies_offline` (kernel crate): the
  result lands as a single replayable `worker.result` bus event with the structured
  payload, receipt-keyed replay is rejected, and the minted worker verifies offline.
