# Artifact lifecycle overlay delta

## MODIFIED Requirements

### Requirement: Activated artifacts MUST survive a kernel restart

An artifact activated into the overlay MUST be reloaded into the registry on the next kernel startup only after its durable learned-artifact provenance exists and the startup compatibility pass succeeds. Base fixtures remain upstream-owned; an overlay file without provenance or with dangling learned references MUST be excluded and surfaced for owner review rather than silently becoming effective.

#### Scenario: Kernel restarts after an activation

Given an artifact was activated into `data/artifacts.d` in a prior kernel run
When the kernel starts up again
Then the artifact MUST be present only if its learned provenance row exists and startup compatibility accepts it
And an overlay without provenance or with dangling learned references MUST be excluded and surfaced for owner review.

## ADDED Requirements

### Requirement: Learned artifacts MUST carry durable exchange provenance

Every activated learned overlay artifact MUST have a non-null producing event identifier and encrypted exchange digest before it becomes visible in the registry.

#### Scenario: Activation without provenance is rejected

Given an artifact activation has no source event or exchange digest
When the kernel attempts to record the learned artifact
Then the store MUST reject the write
And the artifact MUST NOT become effective.

### Requirement: Compatibility MUST fail closed for dangling learned references

After a base update, the kernel MUST validate learned route and workflow references against the merged registry to a fixed point. A dangling reference MUST create a pending re-confirmation and MUST exclude the learned artifact and its learned dependents from effective authority composition.

#### Scenario: Base update orphans a learned route

Given an active learned route references a base agent
When an update removes that agent
Then the compatibility pass MUST record re-confirmation for the route
And the route MUST be absent from the effective registry until reviewed.

### Requirement: Upstream nomination MUST be explicit and opt-in

A learned overlay artifact MAY be nominated as an upstream candidate only through a normal digest-bound review whose request explicitly asserts that the content is depersonalized. Nomination MUST NOT change the artifact namespace automatically.

#### Scenario: Personal artifact cannot be nominated implicitly

Given a learned overlay artifact
When a nomination request omits or falsifies the depersonalized opt-in
Then the kernel MUST reject the request
And the artifact MUST remain an overlay artifact.

### Requirement: Overlay version cutover MUST be highest-only and monotonic

For every artifact kind, proposal and activation MUST reject exact duplicates and lower versions. Activating a higher version MUST atomically replace the prior live version, append an `artifact.superseded` audit, and expose the same highest-only registry after restart.

#### Scenario: Highest version wins across two boots

Given overlay versions v1 and v2 for one identity
When v2 activates and the kernel boots twice
Then only v2 participates in authority on both boots
And the prior v1 is recorded as superseded.

### Requirement: Legacy overlay migration is discovery/quarantine only

`LegacyMigration` is a quarantine placeholder only. When an overlay file has no learned-artifact row, startup MUST synthesize `LegacyMigration` provenance using the actual discovered path and bytes and require digest-bound owner reconfirmation before exposure. A successful owner tap MUST mint a fresh digest-bound proposal and establish `ProducedBy` exchange provenance (producing event id + encrypted exchange digest) BEFORE the artifact becomes visible; the kernel MUST NOT activate an artifact whose effective provenance remains `LegacyMigration`.

#### Scenario: Legacy tap establishes ProducedBy before visibility

Given a quarantined overlay with `LegacyMigration` provenance pending owner review
When the owner reconfirms the exact reviewed bytes
Then the accepted row carries fresh `ProducedBy` exchange provenance and a ReconfirmAnchor
And a fresh proposal is minted and advanced to `Active`
And the artifact is visible only under `ProducedBy` provenance.

#### Scenario: Non-canonical legacy filename survives review

Given a valid learned overlay stored at a non-canonical YAML filename
When startup quarantines and the owner reconfirms it
Then the reviewed bytes are read from that actual source path and the artifact is restored.

### Requirement: Base and overlay namespace collisions MUST refuse replacement

An overlay identity colliding with a base identity MUST remain excluded from authority and owner reconfirmation MUST refuse to replace the base artifact. The base artifact MUST survive both the collision boot and the next boot.

#### Scenario: Collision cannot delete base authority

Given base and overlay artifacts share a kind and id
When the kernel boots twice
Then the overlay is pending owner review and the base remains active on both boots.

### Requirement: Owner acceptance MUST bind to the reviewed base epoch

OwnerAccepted MUST persist the sorted active-base kind/id/version/content-reference epoch and reconfirm provenance, and MUST record a `ReconfirmAnchor` (request id, grant event id, reviewed bytes ref) for every successful reconfirmation regardless of provenance kind. When the base epoch changes, the kernel MUST revalidate the overlay's typed dependencies: compatible overlays refresh the stored epoch without prompting, while newly dangling references are excluded and receive a new pending review. An unchanged restart MUST retain acceptance.

#### Scenario: Base epoch change only prompts newly dangling overlays

Given an owner-accepted overlay and a recorded base compatibility epoch
When an unrelated active base artifact changes
Then the overlay remains owner-accepted and its epoch is refreshed
but when a referenced base artifact changes incompatibly
Then the overlay is excluded and a new digest-bound reconfirmation is pending.

### Requirement: Owner reconfirmation MUST commit atomically before publication

The action request consumption, learned-row `OwnerAccepted` update, matching proposal `Approved -> Active` transition, and acceptance/activation/superseded audits MUST all commit in a single transaction BEFORE the artifact is published to the live registry. A failed or rolled-back commit MUST leave the registry unchanged, the action request retryable, and MUST NOT emit a success audit or owner notification. A concurrent or duplicate tap that loses the consume race MUST publish nothing.

#### Scenario: Failed commit is retryable and publishes nothing

Given a pending overlay reconfirmation
When the durable transaction fails during the owner tap
Then the live registry is unchanged, no success audit is emitted, and the owner may tap again to retry.
