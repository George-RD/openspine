# Global daily spend kill switch

## Why
Per-task grant budgets do not stop aggregate model and connector usage. AD-143 requires a durable kernel-wide daily boundary so a runaway proactive or headless lane cannot continue consuming resources.

## What Changes
- Add configurable daily model-call and connector-call caps.
- Persist UTC-day counters with atomic reserve/check behavior.
- Gate lane admission above grant composition/dispatch: non-immediate lanes pause after breach, while the immediate owner lane receives a truthful owner notification.
- Preserve existing per-task `max_model_calls` budgets.

## Acceptance Criteria
A simulated non-immediate lane cannot compose or dispatch after a daily cap breach, and an immediate breach emits an owner notification under deterministic test.
