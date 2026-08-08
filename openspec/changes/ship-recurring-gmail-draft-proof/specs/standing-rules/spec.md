## MODIFIED Requirements

### Requirement: Exactly one compatible scoped rule MUST match before any budget moves

Scoped admission MUST select over every rule active for the resolved context's action. A rule is compatible only when BOTH its bound compatibility epoch AND its reviewed scope equal those of the freshly resolved context. Selection MUST complete before quota or rate is reserved and before any dark-window timer is scheduled. Exactly one compatible rule MUST admit the action. Zero compatible rules MUST fall back to ordinary owner approval. Two or more compatible rules MUST fail closed: the kernel MUST NOT break the tie by recency, narrowness, or ordering, because any such rule is a policy the owner never reviewed. Neither a mismatch nor an ambiguous overlap may consume quota or rate, create a reservation row, or mint a pending dark-window exception. The selected rule identity MUST be bound inside the same immediate transaction that reserves its budget. Before selection and again inside that same `BEGIN IMMEDIATE` transaction, the kernel MUST consult the durable erased-counterparty marker for any bound counterparty. An erased counterparty MUST produce ordinary owner approval, no reservation, and no pending exception.

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

## ADDED Requirements

### Requirement: Erased counterparties MUST not retain scoped authority

The kernel MUST treat a committed `erased_counterparties` marker as a hard admission predicate for every scoped action bound to that identity. The predicate MUST be enforced at normal scoped admission and at fired-token entry, and MUST be re-read inside the transaction that reserves a scoped rule or consumes a fired token. Erasure MUST also revoke every persisted standing rule whose generic reviewed-scope `Counterparty` value names the erased identity, regardless of whether the rule came from miner-produced learned-artifact provenance or an owner review, and MUST stale its unresolved pending exceptions in the same lifecycle transaction. Already-revoked rows MUST remain idempotent.

This requirement does not claim that the authenticated terminal ledger, SQLite marker, and filesystem key cleanup are one transaction, and it does not erase plaintext briefcases, pending protected payloads, `SYSTEM_SCOPE` artifacts, or owner-review rows.

#### Scenario: Owner-approved scope is revoked on erasure

- **WHEN** an active owner-review-created standing rule carries a reviewed counterparty equal to an identity whose erasure marker commits
- **THEN** the rule MUST leave live consultation
- **AND** its unresolved pending exceptions MUST be staled
- **AND** later admission MUST require ordinary owner approval

#### Scenario: Fired-token erasure check is transactional

- **WHEN** a fired-token consumption transaction observes a committed erasure marker for its bound counterparty
- **THEN** it MUST refuse before claiming the token or reserving budget
- **AND** it MUST not dispatch an effect

### Requirement: Pending Gmail writes MUST fence retries before budget reservation

Before a retry of a kernel-resolved Gmail draft request reaches scoped consultation/reservation or the shared executor, the kernel MUST query the durable pending-write row for the stable protected-reference request fingerprint. A matching pending row MUST fall back to ordinary owner approval without another provider write or standing-rule reservation. `DeliveryUnknown` MUST leave the row pending until explicit reconciliation; this is a retry fence and not an exactly-once delivery guarantee.

#### Scenario: Delivery-unknown retry is fenced

- **WHEN** a prior Gmail draft write has a matching pending row with unknown delivery
- **THEN** a retry MUST perform no provider write
- **AND** it MUST reserve no scoped budget
- **AND** owner review MUST remain the next authority step
