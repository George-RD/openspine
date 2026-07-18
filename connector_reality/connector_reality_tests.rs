mod tests {
    use super::super::*;

    fn now() -> Timestamp {
        "2026-01-01T00:00:00Z".parse().expect("valid fixture time")
    }

    #[test]
    fn rate_limit_refills_after_backoff_interval() {
        let start = now();
        let mut bucket = RateLimitBucket::new(
            RateLimitConfig {
                capacity: 1,
                refill_after: Duration::from_secs(2),
            },
            start,
        );
        assert!(bucket.try_acquire(start).is_ok());
        assert_eq!(
            bucket.try_acquire(start).unwrap_err(),
            Duration::from_secs(2)
        );
        assert!(bucket.try_acquire(start + Duration::from_secs(2)).is_ok());
    }

    #[test]
    fn breaker_transitions_closed_open_half_open_closed() {
        let start = now();
        let mut breaker = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 2,
            open_for: Duration::from_secs(10),
        });
        breaker.record_failure(start);
        assert_eq!(breaker.state(), BreakerState::Closed);
        breaker.record_failure(start);
        assert!(matches!(breaker.state(), BreakerState::Open { .. }));
        assert!(breaker.allow(start).is_err());
        assert!(breaker.allow(start + Duration::from_secs(10)).is_ok());
        assert_eq!(breaker.state(), BreakerState::HalfOpen);
        breaker.record_success();
        assert_eq!(breaker.state(), BreakerState::Closed);
    }

    #[test]
    fn open_breaker_blocks_even_after_cooldown_until_probe_recorded() {
        let start = now();
        let mut runtime = ConnectorRuntime::new(
            RateLimitConfig::default(),
            CircuitBreakerConfig {
                failure_threshold: 1,
                open_for: Duration::from_secs(10),
            },
            start,
        );
        runtime.record_failure(start);
        // After cooldown the first probe is admitted and the breaker stays
        // HalfOpen until a result is recorded (concurrent calls fail closed).
        assert!(runtime
            .try_acquire("gmail", start + Duration::from_secs(10))
            .is_ok());
        assert!(runtime
            .try_acquire("gmail", start + Duration::from_secs(10))
            .is_err());
        runtime.record_success();
        assert!(runtime
            .try_acquire("gmail", start + Duration::from_secs(10))
            .is_ok());
    }

    #[test]
    fn rate_limit_when_empty_keeps_breaker_closed() {
        let start = now();
        let mut runtime = ConnectorRuntime::new(
            RateLimitConfig {
                capacity: 1,
                refill_after: Duration::from_secs(100),
            },
            CircuitBreakerConfig::default(),
            start,
        );
        // Drain the single permit, then try again immediately: the bucket is
        // empty, so admission fails as RateLimited WITHOUT ever consulting the
        // breaker. The breaker starts Closed and must stay Closed — a rate
        // limit rejection never strands it in HalfOpen.
        assert!(runtime.try_acquire("telegram", start).is_ok());
        let err = runtime.try_acquire("telegram", start).unwrap_err();
        assert!(matches!(err, ConnectorCallError::RateLimited { .. }));
        assert_eq!(runtime.breaker_state(), BreakerState::Closed);
    }

    #[test]
    fn unsigned_webhooks_are_rejected() {
        let start = now();
        let verifier = WebhookVerifier::new(b"secret".to_vec(), Duration::from_secs(60));
        let envelope = WebhookEnvelope {
            payload: b"{}",
            signature: "",
            idempotency_key: "event-unsigned",
            signed_at: start - Duration::from_secs(1),
        };
        assert_eq!(
            verifier.verify(envelope, start),
            Err(WebhookRejection::MissingSignature)
        );
    }

    #[test]
    fn replayed_webhooks_are_rejected() {
        let start = now();
        let verifier = WebhookVerifier::new(b"secret".to_vec(), Duration::from_secs(60));
        let payload = b"{}";
        let signed_at = start - Duration::from_secs(1);
        let mut envelope = WebhookEnvelope {
            payload,
            signature: "",
            idempotency_key: "event-replay",
            signed_at,
        };
        envelope.signature = verifier
            .signature(signed_at, envelope.idempotency_key, payload)
            .leak();
        assert!(verifier.verify(envelope, start).is_ok());
        assert_eq!(
            verifier.verify(envelope, start),
            Err(WebhookRejection::Replayed)
        );
    }

    #[test]
    fn stale_success_cannot_close_new_half_open_probe() {
        let start = now();
        let mut runtime = ConnectorRuntime::new(
            RateLimitConfig {
                capacity: 10,
                refill_after: Duration::from_secs(1),
            },
            CircuitBreakerConfig {
                failure_threshold: 2,
                open_for: Duration::from_secs(30),
            },
            start,
        );
        let old_generation = runtime.try_acquire_with_generation("gmail", start).unwrap();
        let failure_one = runtime.try_acquire_with_generation("gmail", start).unwrap();
        let failure_two = runtime.try_acquire_with_generation("gmail", start).unwrap();
        runtime.record_outcome(failure_one, false, start);
        runtime.record_outcome(failure_two, false, start);
        let probe_generation = runtime
            .try_acquire_with_generation("gmail", start + Duration::from_secs(31))
            .unwrap();
        assert_ne!(old_generation, probe_generation);
        runtime.record_outcome(old_generation, true, start + Duration::from_secs(31));
        assert!(matches!(runtime.breaker_state(), BreakerState::HalfOpen));
    }

    #[test]
    fn timeout_outcomes_count_as_breaker_failures() {
        let start = now();
        let mut runtime = ConnectorRuntime::new(
            RateLimitConfig::default(),
            CircuitBreakerConfig {
                failure_threshold: 2,
                open_for: Duration::from_secs(30),
            },
            start,
        );
        // The wrapper records a timeout as a failed connector outcome.
        runtime.record_failure(start);
        assert_eq!(runtime.breaker_state(), BreakerState::Closed);
        runtime.record_failure(start);
        assert!(matches!(runtime.breaker_state(), BreakerState::Open { .. }));
    }

    #[test]
    fn rate_buckets_are_isolated_per_connector() {
        let start = now();
        let config = RateLimitConfig {
            capacity: 1,
            refill_after: Duration::from_secs(60),
        };
        let mut telegram = ConnectorRuntime::new(config, CircuitBreakerConfig::default(), start);
        let mut gmail = ConnectorRuntime::new(config, CircuitBreakerConfig::default(), start);
        assert!(telegram.try_acquire("telegram", start).is_ok());
        assert!(telegram.try_acquire("telegram", start).is_err());
        assert!(gmail.try_acquire("gmail", start).is_ok());
    }

    #[test]
    fn invalid_webhook_signature_does_not_poison_valid_key() {
        let start = now();
        let verifier = WebhookVerifier::new(b"secret".to_vec(), Duration::from_secs(60));
        let payload = b"{\"ok\":true}";
        let signed_at = start - Duration::from_secs(1);
        let invalid = WebhookEnvelope {
            payload,
            signature: "00",
            idempotency_key: "event-invalid",
            signed_at,
        };
        assert_eq!(
            verifier.verify(invalid, start),
            Err(WebhookRejection::InvalidSignature)
        );
        let valid = WebhookEnvelope {
            payload,
            signature: verifier
                .signature(signed_at, "event-invalid", payload)
                .leak(),
            idempotency_key: "event-invalid",
            signed_at,
        };
        assert!(verifier.verify(valid, start).is_ok());
    }

    #[test]
    fn webhook_outside_replay_window_is_rejected() {
        let start = now();
        let verifier = WebhookVerifier::new(b"secret".to_vec(), Duration::from_secs(60));
        let payload = b"{}";
        let signed_at = start - Duration::from_secs(120);
        let envelope = WebhookEnvelope {
            payload,
            signature: verifier.signature(signed_at, "event-2", payload).leak(),
            idempotency_key: "event-2",
            signed_at,
        };
        assert_eq!(
            verifier.verify(envelope, start),
            Err(WebhookRejection::OutsideReplayWindow)
        );
    }
}
