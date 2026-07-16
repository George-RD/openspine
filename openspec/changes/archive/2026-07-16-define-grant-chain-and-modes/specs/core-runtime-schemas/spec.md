# Spec: Core runtime schemas

## MODIFIED Requirements

### Requirement: Task grants MUST be explicit live authority objects

Task grants MUST be short-lived, purpose-bound, route-bound, agent-bound,
workflow-bound, and target-bound where applicable. Running agents and workflows
MUST receive a task grant rather than broad permissions.

A task grant MUST carry an authenticated Macaroons-simple `chain` of ordered
`GrantChainStep` records. Each step contains its `grant_id`, optional
`parent_grant_id`, `mode`, selection-token bindings, and only the caveats added
at that hop. The chain tip `caveat_mac` authenticates the immutable root
authority and every ordered hop. Roots have one empty-caveat step; children
append one step derived from the parent's terminal MAC. `mode` is `live` or
`shadow` (default `live`). Caveat kinds include action allowlists, AD-036 bound
parameters, earlier expiry, model tier, and output-channel allowlists.

The chain is the attenuation proof; a child MUST NOT expand effective actions,
selection tokens, output channels, or execution mode relative to prior hops.
A sub-grant is still a task grant — the only live authority object presented to
a worker (D-007); its parent is lineage only.

#### Scenario: Root grant defaults

Given a newly composed root task grant
When it is inspected
Then its chain has one root step with no parent and no added caveats
And its `caveat_mac` is valid under the kernel-owned verification key.

#### Scenario: Sub-grant is the sole presented authority

Given a parent grant and an attenuated child with a chained delegation step
When a worker starts
Then it receives the child task grant only
And the parent is not a second live authority source.

#### Scenario: Bound parameters are caveats

Given an effectful call has an identity- or scope-bearing parameter
When authority is materialised
Then the binding is represented by a `bound_parameter` caveat
And conflicting values for the same name are rejected as caveat widening.
