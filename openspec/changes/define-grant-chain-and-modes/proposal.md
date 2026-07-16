# Proposal: Define grant chain and modes

## Summary

Add Macaroons-simple authenticated caveat chains and live/shadow mode to task
grants, with pure offline HMAC verification in `gate()`. Shadow success is a
non-executable `effect_suppressed` decision. Runtime sub-grant minting is out
of scope.

## What Changes

- Schema: `root_grant_id`, `parent_grant_id`, `mode`, ordered `caveats`,
  `caveat_mac`; chained HMAC over immutable root authority + caveats +
  instance bind.
- Gate: offline MAC verify; evaluate root ∩ caveats; deny `caveat_widening`
  on failure; map shadow allow/approval to `GateDecision::EffectSuppressed`.
- Dispatch already executes only on `Allow` — `EffectSuppressed` is non-
  executable; kernel test proves no effect handler runs for shadow grants.
- AD-036 bound parameters are caveats. D-007: presented grant is sole live
  authority; parent is lineage only.

## Non-goals

Runtime minting, Biscuit/Datalog, revocation lists, parent DB lookups.

## Acceptance Criteria

Strict OpenSpec validate; MAC/tamper/reorder tests; shadow → EffectSuppressed
with dispatch non-execution test; `./scripts/check.sh` green.
