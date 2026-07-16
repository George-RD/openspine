## ADDED Requirements

### Requirement: Authority-bearing proposals require overlay evaluation before approval
The kernel MUST run offline replay against a provenance-filtered captured owner-control history and an adversarial risk-judge pass before an authority-bearing proposal reaches `review_required` or exposes an owner approval tap. Both passing verdicts MUST be digest-bound to the stored proposal and persisted in the eval-verdict store. If the history or either evaluator is unavailable, the proposal MUST remain outside the approval surface.

#### Scenario: Proposal with two passing evaluations reaches approval
- **GIVEN** a validated route, agent, workflow, pack, or policy proposal
- **AND** captured owner-control history is available
- **WHEN** replay and risk-judge evaluators pass for the stored YAML digest
- **THEN** both verdicts are persisted and the proposal transitions to `review_required`
- **AND** the owner approval summary includes evaluation evidence

#### Scenario: Proposal without captured owner history is denied
- **GIVEN** an authority-bearing proposal with no provenance-filtered owner-control history
- **WHEN** the overlay evaluation gate runs
- **THEN** the proposal does not reach `review_required`
- **AND** no owner approval button is sent

#### Scenario: Generic lifecycle bypass is rejected
- **GIVEN** code attempts a direct `validated` to `review_required` mutation or inserts a proposal already in `review_required`
- **WHEN** the store boundary handles the operation
- **THEN** it rejects the operation because only the digest-bound evaluation promotion can expose approval
