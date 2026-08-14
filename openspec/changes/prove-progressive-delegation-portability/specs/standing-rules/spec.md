## MODIFIED Requirements

### Requirement: Exactly one compatible scoped rule MUST match before any budget moves


Scoped admission MUST select over every rule active for the resolved context's action. A rule is compatible only when BOTH its bound compatibility epoch AND its reviewed scope equal those of the freshly resolved context. Selection MUST complete before quota or rate is reserved and before any dark-window timer is scheduled. Exactly one compatible rule MUST admit the action. Zero compatible rules MUST fall back to ordinary owner approval. Two or more compatible rules MUST fail closed: the kernel MUST NOT break the tie by recency, narrowness, or ordering, because any such rule is a policy the owner never reviewed. Neither a mismatch nor an ambiguous overlap may consume quota or rate, create a reservation row, or mint a pending dark-window exception. The selected rule identity MUST be bound inside the same immediate transaction that reserves its budget, so a concurrent activation cannot swap the rule between selection and reservation. Before selection and again inside that same `BEGIN IMMEDIATE` transaction, the kernel MUST consult the durable erased-counterparty marker for any bound counterparty. An erased counterparty MUST produce ordinary owner approval, no reservation, and no pending exception.

Selection is over rules active for the resolved context's action, so two communication shapes can never contend for one rule. Within a shape, a rule reviewed for one channel, one direct-message counterparty, or one participant set MUST NOT admit another: the visibility values bound through `EffectDestination`, `OutputChannel`, and `BoundParameters` participate in exact-one selection like every other reviewed dimension. Cross-shape, cross-account, cross-target, and cross-visibility confusion MUST each fall back to ordinary owner approval without consuming quota or rate, creating a reservation row, or minting a pending exception.

#### Scenario: Exactly one rule matches

- **WHEN** one active rule's compatibility epoch and reviewed scope both equal the resolved context's
- **THEN** that rule MUST admit the action
- **AND** its quota and rate MUST be reserved only after selection completed
- **AND** the admission evidence MUST record the admitting rule id, its version, and both bound digests

#### Scenario: No rule matches the resolved scope

- **WHEN** no active rule for the action has a reviewed scope equal to the resolved context's
- **THEN** admission MUST fall back to ordinary owner approval
- **AND** no reservation row MUST exist
- **AND** no dark-window timer MUST be scheduled

#### Scenario: Ambiguous overlap fails closed

- **WHEN** two active rules for one action both match the same resolved context
- **THEN** scoped admission MUST be refused
- **AND** no quota or rate MUST be consumed and no reservation row MUST exist
- **AND** no dark-window timer MUST be scheduled and no pending exception MUST be minted
- **AND** the action MUST fall back to ordinary owner approval
- **AND** the refusal MUST leave durable owner-actionable evidence that two approved responsibilities collide

#### Scenario: Two accounts cannot form one pattern

- **WHEN** two resolved contexts for one action differ only in bound account identity, connector instance, counterparty, or canonical target
- **THEN** their `reviewed_scope_digest` values MUST differ
- **AND** a rule reviewed for one MUST NOT admit the other
- **AND** approval evidence gathered against one MUST NOT support the other

#### Scenario: Erasure is checked before scoped reservation

- **WHEN** a bound counterparty has an `erased_counterparties` marker before scoped admission
- **THEN** the request MUST fall back to ordinary owner approval
- **AND** no standing-rule budget or pending exception MUST be created

#### Scenario: Erasure wins the admission transaction race

- **WHEN** the erasure marker commits before the scoped selection/reservation transaction reaches its authoritative predicate
- **THEN** the transaction MUST refuse scoped admission
- **AND** it MUST NOT reserve quota/rate or dispatch an effect

#### Scenario: A rule reviewed for one channel does not admit another

- **WHEN** a second-shape request resolves a channel that no active rule's reviewed `OutputChannel` names
- **THEN** zero rules MUST match and admission MUST fall back to ordinary owner approval
- **AND** no quota, rate, reservation row, or pending exception may move

#### Scenario: A direct-message rule does not admit a channel-visible effect

- **WHEN** a rule reviewed for a direct message is consulted for a request whose resolved `EffectDestination` is a shared channel
- **THEN** it MUST NOT be compatible
- **AND** admission MUST fall back to ordinary owner approval before any effect

