# Design

## Ledger
`store/spend` owns a `daily_spend` table keyed by UTC day. SQLite `INSERT ... ON CONFLICT DO UPDATE` increments each counter under the store mutex. Model reservations enforce the configured model cap; connector reservations enforce the connector cap while allowing the immediate owner-notification path to remain available with an effectively unbounded cap. A separate breach marker makes notification once-per-day and survives restart.

## Boundary
`pipeline::driver::run_pipeline` invokes the spend admission gate before grant composition for the selected lane. `meditate_and_dispatch_action` applies the same gate before dispatch. Lane classification treats `OwnerControl` as immediate and all other lanes as non-immediate, including the test-only scheduled lane used to model future proactive/headless lanes.

## Model and connector accounting
Model calls are reserved immediately before the provider call. Connector sends use the shared connector guard at dispatch and notification/retry call sites; the owner notification path is best-effort and never recursively re-enters the breach notifier. Existing per-task grant counters remain unchanged.

## Configuration
`spend_cap` is a required deny-unknown-fields configuration block with numeric `model_calls_per_day` and `connector_calls_per_day` fields. Example configurations provide finite documented defaults.
