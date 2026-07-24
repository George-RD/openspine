# reflection-miner Specification

## Purpose
TBD - created by archiving change implement-reflection-miner. Update Purpose after archive.
## Requirements
### Requirement: Miner admission MUST use an ordinary bounded grant

The reflection miner MUST require an authenticated ordinary task grant with empty output channels, a pack-derived classification ceiling, bounded model/artifact limits, and model generation admitted through the existing gateway and gate. The miner MUST reject direct activation, policy, or standing-rule mutation actions.

#### Scenario: Ordinary miner grant has no egress

Given an ordinary miner grant with `model.generate:approved_provider` and empty output channels
When the kernel admits the miner
Then the miner MUST be admitted only with the pack classification ceiling and declared limits
And a grant with output channels or direct mutation actions MUST be rejected.

Test: `ordinary_grant_has_empty_output_channels_and_scoped_audit_slice`, `miner_grant_rejects_output_channels`, `miner_grant_rejects_direct_mutation_actions`, `classification_ceiling_is_derived_from_pack_not_caller`

### Requirement: Miner budgets MUST be durably reserved by the kernel

Model and artifact reservations MUST be enforced by the kernel's durable store counters in `BEGIN IMMEDIATE` transactions. A caller-supplied count MUST NOT establish remaining budget.

#### Scenario: AD-135 route persists a bounded proposal

Given a persisted authenticated miner grant and a kernel-packed audit slice
When the runtime mines an owner correction
Then the artifact reservation MUST be durable before lifecycle dispatch
And the proposal MUST enter the normal review-required lifecycle.

Test: `reflection_miner_runtime_wires_ad135_route_through_lifecycle`

### Requirement: Miner model calls MUST pass gateway and gate boundaries

A miner model request MUST be created only after both the gateway and gate have
admitted it, and the request count MUST remain below the ordinary grant
model-call limit. The kernel runtime owns the durable reservation; this change
does not introduce a provider call in the pure schemas boundary.

#### Scenario: Model admission remains kernel-owned

Given an admitted miner grant with `model.generate:approved_provider`
When the scheduled runtime prepares a model request
Then it MUST use the existing gateway and gate admission path
And no direct provider call may originate in the miner.

Test: `reflection_miner_runtime_wires_ad135_route_through_lifecycle`

### Requirement: Briefcase context MUST be scoped and provenance-bound

The miner MUST receive only a read-only audit-trail slice belonging to its grant and scope. Every observation MUST match a briefcase entry's event ID and encrypted exchange reference exactly.

#### Scenario: Out-of-scope observation is rejected

Given a miner briefcase containing one scoped audit entry
When an observation refers to a different event or encrypted exchange
Then mining MUST reject the observation before producing a proposal.

Test: `miner_rejects_observation_outside_scoped_audit_slice`

### Requirement: Miner proposals MUST carry encrypted source provenance

Every proposal produced by the miner MUST carry the source event ID and encrypted exchange reference that caused the proposal.

#### Scenario: Proposal provenance survives mining

Given a scoped observation with an encrypted exchange provenance
When the miner emits a proposal
Then the proposal MUST carry the exact event and exchange reference
And its lifecycle state MUST remain `proposed`.

Test: `every_miner_proposal_carries_encrypted_exchange_provenance`, `miner_cannot_write_kernel_state_and_only_returns_proposed_rows`

### Requirement: Corrections MUST use positive instruction rewrites

A correction with reasons MUST become an instruction rewrite proposal. The miner MUST NOT append a prohibition artifact.

#### Scenario: Correction produces a rewrite

Given an owner correction with a positive replacement instruction and reason
When the miner mines the correction
Then the output MUST be an instruction rewrite carrying the reason
And the output MUST remain lifecycle-proposed.

Test: `correction_rewrites_instruction_and_negative_constraint_becomes_probe`, `correction_never_appends_a_prohibition_artifact`

### Requirement: Negative constraints MUST become eval probes

A negative constraint attached to a correction MUST become structured eval-probe data and MUST NOT be inserted into persona guidance as a prohibition.

#### Scenario: Negative correction constraint becomes a probe

Given a correction containing a negative constraint
When the miner produces its rewrite proposal
Then the proposal MUST carry an eval probe for a scenario satisfying the rewrite
And no prohibition append body MUST exist.

Test: `correction_rewrites_instruction_and_negative_constraint_becomes_probe`, `correction_never_appends_a_prohibition_artifact`

### Requirement: Prohibition-shaped rewrites MUST be rejected

The positive-steering boundary MUST reject an instruction whose own text is a
prohibition. Negative constraints may remain only as `EvalProbe` data.

#### Scenario: Prohibition-shaped instruction is not emitted

Given a correction instruction beginning with `Do not`, `Never`, or equivalent
When the miner mines the correction
Then it MUST reject the correction
And it MUST emit no prohibition artifact.

Test: `correction_rejects_prohibition_shaped_instruction`

### Requirement: Repeated approvals MUST be derived from scoped audit evidence

Repeated approvals MAY produce a standing-rule candidate only when at least two
kernel-packed audit entries reference the same approved artifact. The observed
action ID MUST be carried into the candidate; no caller-supplied approval count
or hardcoded action is permitted.

#### Scenario: Repeated approvals remain proposed

Given two allowed audit entries for one approved artifact in the scoped slice
When the miner emits an output
Then the output MUST be a proposed standing-rule candidate
And it MUST preserve the observed action ID without activating authority.

Test: `repeated_approval_is_only_a_standing_rule_candidate`, `repeated_approval_requires_kernel_verifiable_evidence`

### Requirement: Consolidation MUST be lifecycle-safe

A scheduled consolidation/autophagy pass MUST emit a proposed merge/prune operation rather than directly merging or deleting learned artifacts.

#### Scenario: Consolidation remains a proposal

Given merge and prune targets from the learned-artifact backlog
When consolidation runs
Then it MUST emit a proposed consolidation output
And it MUST NOT immediately mutate learned artifacts.

Test: `consolidation_is_a_proposal_not_an_immediate_prune_or_merge`

### Requirement: AD-135 digest corrections MUST enter the persona proposal lifecycle

An owner correction to `digest_brief_default` MUST become a persona instruction rewrite and enter the normal `artifact.propose` lifecycle as a reviewable proposed row. Activation remains owner-approved and grant-provenance-bound.

#### Scenario: Digest default correction is proposed

Given a miner correction targeting `digest_brief_default`
When the miner adapter submits its persona payload through `artifact.propose`
Then the kernel MUST persist a `persona` proposed-artifact row for owner review
And the row MUST not be active before approval.

Test: `digest_default_owner_correction_uses_normal_persona_proposal_route`, `artifact_propose_accepts_persona_kind`, `artifact_propose_accepts_miner_reflection_correction_route`

### Requirement: Miner output classes MUST be explicit

The miner MUST classify outputs as corrections-with-reasons, repeated approvals, stated preferences, or consolidation and MUST preserve the normal lifecycle boundary for every class.

#### Scenario: Stated preference is emitted as an overlay proposal

Given a stated owner preference
When the miner emits an output
Then the output class MUST be stated preference and lifecycle-proposed
And it MUST carry source provenance.

Test: `miner_cannot_write_kernel_state_and_only_returns_proposed_rows`, `persona_proposal_serializes_to_normal_lifecycle_payload`

