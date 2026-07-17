## ADDED Requirements

### Requirement: Global daily spend ledger
The kernel MUST persist UTC-day model-call and connector-call counters separately from per-task grant budgets and update each counter atomically at its usage boundary.

#### Scenario: Counters persist by UTC day
- **WHEN** model or connector usage is reserved and the process restarts
- **THEN** the same UTC-day ledger contains the increment, while a new UTC day starts with zero counters

#### Scenario: Concurrent reservation cannot overspend
- **WHEN** multiple callers reserve usage against a finite daily cap
- **THEN** SQLite atomic update semantics admit no reservation that crosses the configured cap

### Requirement: Lane-aware breach boundary
The kernel MUST evaluate the global daily cap before grant composition and dispatch, pause non-immediate lanes after breach, and preserve the immediate owner lane for notification.

#### Scenario: Non-immediate lane is paused
- **WHEN** a simulated scheduled, proactive, or headless lane reaches a breached daily cap
- **THEN** grant composition and dispatch are denied and no work is executed

#### Scenario: Immediate owner notification
- **WHEN** the first non-immediate breach is detected
- **THEN** the owner is notified through the truthful `notify_owner_required` path and the breach is durably marked once per UTC day

### Requirement: Configurable caps
The configuration MUST expose finite numeric `model_calls_per_day` and `connector_calls_per_day` fields in a deny-unknown-fields `spend_cap` block.

#### Scenario: Configured caps are enforced
- **WHEN** an operator supplies `spend_cap` in config.yaml
- **THEN** the kernel enforces those values without changing existing per-task grant budget behavior
