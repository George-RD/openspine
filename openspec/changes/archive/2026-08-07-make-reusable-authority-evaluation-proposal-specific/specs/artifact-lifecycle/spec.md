# Artifact lifecycle

## MODIFIED Requirements

### Requirement: Authority-bearing proposals require overlay evaluation before approval
Every authority-bearing proposal MUST pass a digest-bound replay and adversarial risk-judge evaluation before reaching `review_required` or exposing an owner approval tap. Both evaluators MUST run against a kernel-assembled evaluation input bound to the exact stored proposal digest; a dimension the kernel cannot resolve MUST produce a typed incomplete-input denial naming that dimension, and MUST NOT be treated as a generic artifact or a pass.

A **reusable-authority proposal** — one that proposes a standing rule admitting an action without per-instance owner approval — MUST additionally satisfy a structural judge. For every such proposal the judge MUST establish that the action exists in the canonical catalog, that it is not catalogued as non-delegable, and that no composed policy denies it. A proposal whose action carries no reusable-delegation descriptor grants blanket authority, so it MUST additionally be an action the catalog declares approval-narrowing — one that narrows an approval requirement rather than admitting an effect; any other action MUST bind a reviewed scope before a standing rule may admit it. Where the action carries a reusable-delegation descriptor, the judge MUST additionally establish that its named implementation has a registered executor and a declared resolver, that the declared delegation contract is eligible, that every action-required reviewed-scope dimension is present and internally consistent, that the effect class and any dark-window configuration are admissible for the action, that budgets and expiry lie within the action's declared bounds, and that the proposal neither equals nor widens the reviewed scope of an active rule held by a different artifact. Its replay pass MUST execute concrete cases against the exact proposed binding and record their outcomes; an evaluation that executed no cases, or whose executed cases do not include both a matching case and a refused changed-context case, MUST be a denial. A standing rule whose action carries no reusable-delegation descriptor has no reviewed scope to vary, so its replay MUST fall to the availability check and MUST NOT be reported as a replay.

Any other authority-bearing proposal MUST still pass structural probes over the canonical catalog and its declared authority lists, and a `model_swap` proposal MUST use kernel-executed golden-set replay as its executed-case evidence. Where an evaluator measures only availability or corpus presence, it MUST be named for what it measures and MUST NOT be reported as a replay. In all paths, both passing verdicts MUST bind the exact stored proposal digest and persist in the eval-verdict store.

#### Scenario: Proposal with two passing evaluations reaches approval
- **GIVEN** a validated authority-bearing proposal whose evaluation input resolves completely
- **WHEN** both evaluators pass for the stored proposal digest
- **THEN** both verdicts are persisted and the proposal transitions to `review_required`
- **AND** the owner approval summary includes the evaluation evidence

#### Scenario: Reusable-authority proposal whose action has no registered executor is denied
- **GIVEN** a standing-rule proposal naming an implementation with no registered executor
- **WHEN** the overlay evaluation gate runs
- **THEN** the gate MUST deny with an executor-readiness reason
- **AND** the proposal MUST NOT reach `review_required`
- **AND** no verdict recording a pass MUST be persisted

#### Scenario: Proposal with an incomplete evaluation input is denied by dimension
- **GIVEN** a standing-rule proposal whose reviewed-scope binding omits a dimension the action requires, or whose stored scope digest disagrees with its stored values
- **WHEN** the kernel assembles the evaluation input
- **THEN** assembly MUST fail with an incomplete-input denial naming the missing or inconsistent dimension
- **AND** the proposal MUST NOT be evaluated as a generic artifact

#### Scenario: Policy-denied or out-of-bounds reusable-authority proposal is refused
- **GIVEN** a standing-rule proposal for an action the composed policy denies, or whose budgets or expiry fall outside the action's declared bounds
- **WHEN** the structural judge runs
- **THEN** the judge MUST deny naming the failing axis
- **AND** the proposal MUST NOT reach `review_required`

#### Scenario: Ambiguously overlapping or widening proposal is refused
- **GIVEN** an active rule for the action and a standing-rule proposal whose reviewed scope equals or widens that rule's reviewed scope
- **WHEN** the structural judge runs
- **THEN** the judge MUST deny as an ambiguous or widening overlap
- **AND** a proposal whose reviewed scope is disjoint from every active rule MUST NOT be denied on that axis

#### Scenario: An evaluation that executed no cases cannot pass as replay
- **GIVEN** a reusable-authority proposal whose replay produced an empty executed-case ledger, or a ledger with no refused changed-context case
- **WHEN** the gate evaluates the replay result
- **THEN** the gate MUST deny
- **AND** the denial MUST NOT be recorded or rendered as a replay pass

#### Scenario: Proposal without captured owner history is denied
- **GIVEN** an authority-bearing proposal other than `model_swap` whose replay falls to the availability check, with no provenance-filtered owner-control history
- **WHEN** the overlay evaluation gate runs
- **THEN** the proposal does not reach `review_required`
- **AND** no owner approval button is sent

#### Scenario: A non-delegable action cannot carry a standing rule
- **GIVEN** a standing-rule proposal for an action the catalog marks non-delegable
- **WHEN** the structural judge runs
- **THEN** the judge MUST deny naming delegability
- **AND** the proposal MUST NOT reach `review_required`

#### Scenario: An uncatalogued action cannot carry a standing rule
- **GIVEN** a standing-rule proposal naming an action id that is not in the canonical catalog
- **WHEN** the structural judge runs
- **THEN** the judge MUST deny as an unknown action
- **AND** the proposal MUST NOT reach `review_required`

#### Scenario: An effectful action cannot carry an unscoped standing rule
- **GIVEN** a standing-rule proposal that binds no reviewed scope, for a catalogued action the catalog does not declare approval-narrowing
- **WHEN** the structural judge runs
- **THEN** the judge MUST deny, requiring a reviewed scope
- **AND** a proposal for an action the catalog does declare approval-narrowing MUST NOT be denied on that axis

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

## ADDED Requirements

### Requirement: Overlay evaluation MUST NOT grant or activate authority
A passing overlay evaluation MUST move a proposal no further than `review_required`. Evaluation MUST NOT mint a task grant, activate or supersede a standing rule, reserve or commit budget, schedule a dark-window pending row, or execute any connector effect. Replay MUST be a pure decision over in-memory candidate bindings.

#### Scenario: A passing evaluation changes no runtime authority
- **WHEN** a reusable-authority proposal passes both evaluators
- **THEN** the proposal MUST be in `review_required`
- **AND** no rule MUST be active for its action as a result
- **AND** no standing-rule usage row, dark-window pending row, or task grant MUST have been created

#### Scenario: Replay causes no external effect
- **WHEN** replay executes its case set for a proposal whose action is a connector write
- **THEN** no connector effect MUST be dispatched
- **AND** no pending-write fence row MUST be created

### Requirement: Gate summary copy MUST state only what executed cases prove
The owner-facing overlay gate summary MUST be derived from the stored verdicts and their recorded evidence, not authored as free text. It MUST report what actually ran, and MUST NOT describe an evaluation as a replay of prior or synthetic cases unless concrete cases were executed and recorded.

#### Scenario: Summary reports executed case counts
- **GIVEN** a reusable-authority proposal whose replay executed evidence-derived matching cases and changed-context cases
- **WHEN** the gate renders the owner summary
- **THEN** the summary MUST state the number and kinds of cases executed
- **AND** every claim in the summary MUST be derivable from the stored verdicts

#### Scenario: No replay claim without executed cases
- **GIVEN** an evaluation whose evidence records no executed cases
- **WHEN** the gate renders any owner-facing copy
- **THEN** that copy MUST NOT assert that prior or synthetic cases were replayed
