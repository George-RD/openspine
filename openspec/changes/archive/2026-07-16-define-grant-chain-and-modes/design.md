# Design: Define grant chain and modes

## Macaroons-simple chain (AD-101)

The grant carries its own self-contained, authenticated attenuation proof. Gate
verifies offline with an HMAC key from `GateContext` — no parent-grant DB
lookup, no hand-written parent/child list intersection against an absent
parent body.

### Immutable root authority + append-only caveats

- Root authority fields on every grant (copied unchanged to children):
  `root_grant_id`, `allowed_actions`, `approval_required_actions`,
  `denied_actions`, `output_channels`, `limits`, `expires_at`.
- For a root grant, `root_grant_id == id` and `parent_grant_id` is `None`.
- For a child, `root_grant_id` is the original root's id, `parent_grant_id` is
  the immediate parent (lineage only; D-007: parent is never a second live
  authority object presented to a worker).
- `caveats: Vec<Caveat>` is the full ordered chain from root to this grant.
  Children only APPEND. Caveat kinds: `action_allowlist`, `bound_parameter`
  (AD-036), `expires_before`, `model_tier`, `output_channel_allowlist`.
- `mode: live | shadow` is bound per grant instance (may differ on a child).

### Chained HMAC construction

```
sig₀ = HMAC-SHA256(key, canonical(root_authority))
sigᵢ = HMAC-SHA256(sigᵢ₋₁, canonical(caveatᵢ))   // for each caveat in order
tip  = HMAC-SHA256(sigₙ, canonical(id, parent_grant_id, mode))
caveat_mac = hex(tip)
```

`root_authority` is the immutable envelope above (not the child-only instance
fields). Recomputing from key + fields + full caveat list verifies the tip
without the parent row. Editing any root authority field, reordering/removing
caveats, or changing id/parent/mode without a matching tip fails verification.

A fresh root-key HMAC over a child alone is **not** used: verification always
replays the full chain from the root commitment through every inherited
caveat. Attenuation restrictions live in caveats over that immutable root;
gate evaluates the request against root lists **intersected with** caveat
semantics (action not in an `action_allowlist` caveat → not granted; past an
`expires_before` → deny; bound parameters are fixed name/value pairs).

### Gate decisions and shadow

- Invalid MAC or caveat-semantic failure → `Deny { caveat_widening }`.
- Otherwise existing precedence (deny > approval-required > allow > not
  granted), applied to **effective** authority (root ∩ caveats).
- If the would-be decision is `Allow` or `ApprovalRequired` and `mode ==
  shadow`, the returned decision is **`EffectSuppressed`** — a distinct,
  non-executable success. Dispatch already matches only `GateDecision::Allow`
  before running effects; `EffectSuppressed` cannot be treated as executable
  success. No advisory boolean.

### Out of scope

Runtime sub-grant minting, key rotation UX, Biscuit/Datalog, revocation lists
(`implement-worker-runtime` owns minting).
