## ADDED Requirements

### Requirement: Verified hooks use the governed pipeline without conversation
A verified webhook MUST enter the ordinary event pipeline and MUST NOT create an owner conversation when composed authority requires no approval; its completion MUST be surfaced through the owner digest.

#### Scenario: Verified hook completes headlessly
- **GIVEN** a signed, replay-protected webhook and a route whose composed authority requires no approval
- **WHEN** the hook traverses verify, identify, route, compose, grant, run, gate, and audit
- **THEN** the flow completes with zero owner conversation messages and one owner-digest item

#### Scenario: Approval-required hook escalates normally
- **GIVEN** a signed, replay-protected webhook whose composed authority marks the selected action approval-required
- **WHEN** the hook reaches gate
- **THEN** the owner notification and digest escalation path is used and no effect runs before approval

#### Scenario: Invalid or replayed hook is dropped
- **GIVEN** a webhook with an invalid signature, stale timestamp, or consumed idempotency key
- **WHEN** the headless verifier runs
- **THEN** the delivery is dropped before event admission, with rejection audit and no task grant

#### Scenario: Route selector is authenticated
- **GIVEN** a valid signature for one channel account
- **WHEN** the same delivery is submitted with a different channel account
- **THEN** verification fails closed and no alternate route can be selected
