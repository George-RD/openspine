# Model swap ceremony Specification

## ADDED Requirements
### Requirement: Model swaps MUST be evidence-bearing AD-142 proposals
A base, matcher, or miner model swap MUST be represented by a `model_swap` proposal carrying a role, an operator-configured provider id, a trusted role-authorized golden-set id, and kernel-generated golden-set replay evidence. Proposer-supplied case results MUST be rejected, and a model swap MUST NOT reach `review_required` without both digest-bound AD-142 verdicts. The digest-bound owner approval summary MUST include the role, target provider, and bounded observed case evidence, MUST fit within 3,500 UTF-16 units, and MUST fail closed without exposing an approval tap if that bound cannot be met.

#### Scenario: Missing evidence is kernel-enriched
- **GIVEN** a `model_swap` YAML omits `golden_set_result`
- **WHEN** `artifact.propose` is dispatched
- **THEN** the kernel MUST run the trusted role-authorized golden set and attach bounded kernel-observed evidence before persistence or owner approval
#### Scenario: Proposer-supplied evidence is denied
- **GIVEN** a `model_swap` YAML includes `golden_set_result`
- **WHEN** `artifact.propose` is dispatched
- **THEN** the kernel MUST reject it before persistence or owner approval
#### Scenario: Candidate is evaluated against trusted immutable cases
- **GIVEN** a proposal names a configured provider and role-authorized golden set
- **WHEN** proposal enrichment runs
- **THEN** the kernel MUST call the candidate for every bounded case
- **AND** MUST derive `passed` from deterministic `must_contain`/`must_not_contain` checks
- **AND** MUST persist bounded observed excerpts, full-output digests, case ids/kinds, golden-set digest, and provider-config digest

#### Scenario: Oversized approval summary fails closed
- **GIVEN** the role, target provider, and bounded observed evidence cannot fit within 3,500 UTF-16 units
- **WHEN** the kernel builds the digest-bound owner approval summary
- **THEN** the proposal MUST remain outside the approval surface and no approval tap may be sent

### Requirement: Golden sets MUST be bounded and role-bound
Golden sets MUST be fixture-only YAML with `id: string`, `schema_version: u32`, `roles: [base|matcher|miner]`, optional `system: string`, and `cases: [{id: string, kind: standard|adversarial, prompt: string, must_contain: [string], must_not_contain: [string]}]`. The corpus MUST have at least three standard and one adversarial case, at most 20 cases, at most 4,000 UTF-8 bytes each for `system` and `prompt`, at most 500 bytes per criterion, at most 10 criteria per case, and at most 500 bytes per observed excerpt. Pass/fail MUST be case-sensitive: every `must_contain` string MUST occur and no `must_not_contain` string MAY occur; standard coverage passes with at least three passing standards and adversarial coverage requires every adversarial case to pass.
Replay MUST end at the lesser of five minutes and the proposing grant's remaining wall-clock expiry. Every attempted provider call MUST consume an atomically reserved model-call budget unit even when the call, replay, or overall run fails or times out.

#### Scenario: Failed adversarial coverage is denied
- **GIVEN** a golden-set run has fewer than three passing standard cases or a failing adversarial case
- **WHEN** the AD-142 model-swap gate runs
- **THEN** the proposal MUST remain outside `review_required`

#### Scenario: Malformed golden set fails closed
- **GIVEN** a fixture has missing role authorization, insufficient coverage, duplicate case ids, or exceeds a cap
- **WHEN** the kernel loads fixtures
- **THEN** startup MUST fail with a validation error

#### Scenario: Replay is bounded by the grant
- **GIVEN** the fixed five-minute replay cap exceeds the grant's remaining expiry
- **WHEN** golden-set replay runs
- **THEN** the kernel MUST stop no later than the grant expiry and MUST keep the proposal outside `review_required`

#### Scenario: Failed attempts consume call budget
- **GIVEN** a provider call was attempted during golden-set replay
- **WHEN** that call fails or the replay later times out
- **THEN** its atomically reserved model-call budget unit MUST remain consumed

### Requirement: Activation MUST use a serialized provenance-bound staged protocol
The active role-to-provider mapping consumed by the gateway MUST change only during digest-bound approved activation. Model-swap activation MUST write a loader-invisible `.pending` candidate, then transactionally commit monotonic same-role supersession, `Approved` to `Active`, and exactly one `artifact.activated` audit before atomically renaming and publishing the registry/provider mapping under a serialized activation boundary. Activation and restart MUST re-resolve the trusted golden set and provider pool and require both embedded digests to match; missing or changed dependencies MUST fail closed. Generic artifact kinds MUST retain their existing atomic temporary-write path and MUST NOT depend on model-swap recovery.

#### Scenario: Approved Base swap changes the gateway selection
- **GIVEN** an enriched Base swap passes AD-142 and the owner approves its exact YAML
- **WHEN** activation completes
- **THEN** the active Base provider assignment MUST point to the approved provider
- **AND** the real model-generate gateway path MUST call that provider

#### Scenario: Dependency drift blocks activation and restart
- **GIVEN** the trusted golden set or non-secret provider configuration changes after evidence was produced
- **WHEN** activation or startup restores the active swap
- **THEN** the kernel MUST refuse the swap rather than silently selecting the changed provider

#### Scenario: Transaction failure exposes no candidate
- **GIVEN** an approved model-swap candidate is staged
- **WHEN** the lifecycle or activation-audit transaction fails
- **THEN** the pending candidate MUST be removed
- **AND** the prior disk, registry, provider assignment, and proposal state MUST remain authoritative

#### Scenario: Committed pending activation recovers after a crash
- **GIVEN** lifecycle and activation audit committed but the process crashed before the pending file was renamed
- **WHEN** startup verifies the pending canonical bytes against the committed Active proposal digest
- **THEN** startup MUST complete the rename and publish the committed provider

#### Scenario: Uncommitted or tampered pending candidate is not activated
- **GIVEN** a pending candidate has no matching Active proposal or its canonical bytes do not match the committed digest
- **WHEN** startup reconciles pending model swaps
- **THEN** an uncommitted candidate MUST be removed and a tampered candidate MUST be quarantined
- **AND** neither candidate MUST enter the active registry or provider map

### Requirement: Provider configuration changes MUST NOT bypass the ceremony
The kernel MUST treat the active role-to-provider assignment as the only runtime-proposable surface. The configured provider pool and its credentials remain bootstrap-only; changing that pool cannot silently alter an active persisted role assignment because active swaps are digest-bound and revalidated.

#### Scenario: Config-only change cannot silently replace an active role
- **GIVEN** an active swap binds a provider configuration digest
- **WHEN** the configured provider's model, kind, endpoint, or id changes
- **THEN** startup MUST fail closed until the operator restores the bound configuration or activates a newer approved swap

### Requirement: Restart MUST require symmetric latest ceremony provenance
For each role, the exact normalized active overlay manifest MUST match the latest persisted Active proposed-artifact row for the same id and version, its reviewed manifest digest, and both passing digest-bound AD-142 verdicts. The check MUST run in both directions: an overlay without matching latest Active provenance and a latest Active row without its exact active overlay MUST each fail startup. Missing, inactive, downgraded, shadowing, or mismatched overlays MUST NOT cause fallback to an older swap or bootstrap provider.

#### Scenario: Injected active fixture is denied
- **GIVEN** an active model-swap YAML exists without matching persisted ceremony provenance
- **WHEN** the kernel starts
- **THEN** startup MUST fail closed before publishing the active provider assignment

#### Scenario: Missing latest overlay cannot silently roll back
- **GIVEN** the latest persisted Active Base swap is newer than the remaining overlay or bootstrap assignment
- **WHEN** its exact overlay is missing, inactive, shadowed, downgraded, or mismatched at startup
- **THEN** startup MUST fail closed rather than select the older or bootstrap provider

