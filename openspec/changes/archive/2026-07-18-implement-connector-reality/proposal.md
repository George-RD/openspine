# Connector reality hardening

## Why
Connector effects need structural health and authenticity boundaries before hook lanes and additional external integrations land. AD-141 requires connector-scoped rate limiting, proactive credential refresh, signed/replay-safe webhook admission, breaker isolation, and bounded calls. AD-103 explicitly excludes bulkhead pools from this change.

## What Changes
- Add per-connector token-bucket limits with retry backoff and Closed/Open/HalfOpen circuit breakers.
- Refresh Gmail access tokens before expiry while retaining vault-version invalidation.
- Provide a kernel webhook verification substrate with signature, idempotency, and bounded replay-window checks.
- Route Gmail effects through breaker admission and a per-call timeout; Open/HalfOpen rejection emits `connector_unavailable`, distinct from policy denial.

Affected layer: OpenSpine core. Authority-sensitive: connector access, event authenticity, private data, and audit.
