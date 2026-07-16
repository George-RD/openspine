# artifact-lifecycle Specification Delta

## MODIFIED Requirements

### Requirement: Proposed artifacts MUST be schema-validated before persistence
`artifact.propose` MUST parse the proposed YAML against the schema for its declared kind before persisting anything, and MUST reject a kind outside `route | agent | workflow | pack | policy | model_swap`.

#### Scenario: Malformed YAML is rejected
- **GIVEN** a chat proposes YAML that does not parse against its declared kind
- **WHEN** `artifact.propose` is dispatched
- **THEN** the kernel MUST return a bad-request error and MUST NOT persist a proposed-artifact row or send an approval button.

#### Scenario: An unknown kind is rejected
- **GIVEN** a chat proposes an artifact with `kind` outside the six supported proposable kinds
- **WHEN** `artifact.propose` is dispatched
- **THEN** the kernel MUST return a bad-request error.

### Requirement: Authority-bearing proposals require overlay evaluation before approval
The kernel MUST run offline replay against a provenance-filtered captured owner-control history and an adversarial risk-judge pass before a route, agent, workflow, pack, or policy proposal reaches `review_required` or exposes an owner approval tap. Both passing verdicts MUST be digest-bound to the stored proposal and persisted in the eval-verdict store. If the history or either evaluator is unavailable, the proposal MUST remain outside the approval surface. For a model_swap proposal, kernel-executed golden-set replay replaces captured owner-control history as the replay evidence; its replay and risk-judge verdicts MUST bind the exact stored proposal digest.

#### Scenario: Proposal with two passing evaluations reaches approval
- **GIVEN** a validated route, agent, workflow, pack, or policy proposal
- **AND** captured owner-control history is available
- **WHEN** replay and risk-judge evaluators pass for the stored YAML digest
- **THEN** both verdicts are persisted and the proposal transitions to `review_required`
- **AND** the owner approval summary includes evaluation evidence

#### Scenario: Proposal without captured owner history is denied
- **GIVEN** an authority-bearing route, agent, workflow, pack, or policy proposal with no provenance-filtered owner-control history
- **WHEN** the overlay evaluation gate runs
- **THEN** the proposal does not reach `review_required`
- **AND** no owner approval button is sent

#### Scenario: Generic lifecycle bypass is rejected
- **GIVEN** code attempts a direct `validated` to `review_required` mutation or inserts a proposal already in `review_required`
- **WHEN** the store boundary handles the operation
- **THEN** it rejects the operation because only the digest-bound evaluation promotion can expose approval

#### Scenario: Model swap with two passing evaluations reaches approval
- **GIVEN** a validated model_swap proposal has kernel-generated golden-set evidence
- **WHEN** replay and risk-judge evaluators pass for the stored YAML digest
- **THEN** both verdicts MUST be persisted and the proposal MUST transition to `review_required`
- **AND** the owner approval summary MUST include role, target provider, and bounded observed case evidence.

#### Scenario: Missing model-swap evaluation blocks approval
- **GIVEN** either model-swap evaluator is unavailable or fails
- **WHEN** a model_swap proposal is dispatched
- **THEN** the proposal MUST remain outside the approval surface.

#### Scenario: Model swap lifecycle bypass is rejected
- **GIVEN** code attempts to insert a model_swap proposal already in `review_required` or directly mutate it into `review_required`
- **WHEN** the store boundary handles the operation
- **THEN** it rejects the operation because only the digest-bound replay and risk-judge promotion can expose approval
