# Design

`ConnectorRuntime` owns a token bucket and circuit breaker per registered connector. Admission checks an active breaker before consuming rate capacity, allowing a timed Open breaker to probe only after capacity is reserved. Failures and successes update the breaker deterministically.

Gmail effect dispatch is wrapped after `gate()` returns `Allow`: admission occurs before the connector handler, and `tokio::time::timeout` bounds the whole call. Open/HalfOpen admission failures append `connector_unavailable` and enter connector failure surfacing; they never become policy denials.

Gmail cached access tokens are accepted only while valid with the credential-version pair and are refreshed inside the configured pre-expiry skew. `WebhookVerifier` validates the signed payload, timestamp window, and idempotency key against a bounded in-memory replay set; consumers can later persist/replace the replay set without changing the verification contract.

Bulkhead resource pools are deliberately excluded per AD-103's single-owner rationale.
