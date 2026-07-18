# connector-reality Specification

## Purpose
TBD - created by archiving change implement-connector-reality. Update Purpose after archive.
## Requirements
### Requirement: Connector effects MUST have per-connector admission controls
Each registered connector MUST have an independent rate-limit bucket and circuit breaker. Admission MUST apply rate limits and Closed/Open/HalfOpen state before the effect runs, and a rejected admission MUST NOT invoke the connector.

#### Scenario: Rate-limited connector backs off
- **GIVEN** one connector has exhausted its token bucket
- **WHEN** an effect is admitted
- **THEN** admission is rejected with a retry-after backoff
- **AND** another connector remains independently admissible.

#### Scenario: Open breaker blocks before effect
- **GIVEN** a connector breaker is Open or a HalfOpen probe is already in flight
- **WHEN** `gate()` has allowed the effect and dispatch begins
- **THEN** the effect is blocked before any connector call
- **AND** a `connector_unavailable` audit event is recorded
- **AND** the result is not a policy denial.

### Requirement: Connector calls MUST be bounded
Every connector effect call MUST have a bounded timeout, and a timeout MUST record a connector failure for breaker health without being presented as a policy denial.

#### Scenario: Connector call exceeds timeout
- **GIVEN** `gate()` allowed a connector effect
- **WHEN** the connector call exceeds its configured timeout
- **THEN** dispatch returns a connector failure
- **AND** the breaker records the failed outcome.

### Requirement: Gmail credentials MUST refresh before expiry
A cached Gmail access token MUST be refreshed when it enters the configured pre-expiry skew, while credential-slot version changes MUST continue to invalidate the cache.

#### Scenario: Near-expiry token refreshes
- **GIVEN** a cached Gmail token is still valid but within the refresh skew
- **WHEN** Gmail performs a credentialed call
- **THEN** the kernel refreshes before using the near-expiry token.

### Requirement: Webhook admission MUST reject spoofed and replayed requests
The kernel webhook substrate MUST verify the signature over the received payload, require an idempotency key, and reject timestamps outside the bounded replay window. A key accepted once MUST be rejected on replay.

#### Scenario: Unsigned webhook is rejected
- **GIVEN** a webhook payload has no valid signature
- **WHEN** webhook verification runs
- **THEN** verification rejects the request before dispatch.

#### Scenario: Replayed webhook is rejected
- **GIVEN** a signed webhook with an idempotency key was accepted
- **WHEN** the same key is verified again
- **THEN** verification rejects the replay.

#### Scenario: Stale webhook is rejected
- **GIVEN** a signed webhook timestamp is outside the configured replay window
- **WHEN** webhook verification runs
- **THEN** verification rejects the request before dispatch.

