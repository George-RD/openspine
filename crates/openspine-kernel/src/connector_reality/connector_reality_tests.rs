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
            failure_window: Duration::from_secs(60),
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
        let runtime = ConnectorRuntime::new(
            RateLimitConfig::default(),
            CircuitBreakerConfig {
                failure_threshold: 1,
                open_for: Duration::from_secs(10),
                failure_window: Duration::from_secs(60),
            },
            start,
        );
        runtime.record_failure(start);
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
        let runtime = ConnectorRuntime::new(
            RateLimitConfig {
                capacity: 1,
                refill_after: Duration::from_secs(100),
            },
            CircuitBreakerConfig::default(),
            start,
        );
        assert!(runtime.try_acquire("telegram", start).is_ok());
        assert!(matches!(
            runtime.try_acquire("telegram", start),
            Err(ConnectorCallError::RateLimited { .. })
        ));
        assert_eq!(runtime.breaker_state(), BreakerState::Closed);
    }
    #[test]
    fn invalid_webhook_signature_does_not_poison_valid_key() {
        let start = now();
        let verifier = WebhookVerifier::new(b"secret".to_vec(), Duration::from_secs(60));
        let payload = b"{}";
        let signed_at = start - Duration::from_secs(1);
        let mut envelope = WebhookEnvelope {
            payload,
            signature: "sha256=00",
            idempotency_key: "same-key",
            signed_at,
        };
        assert_eq!(
            verifier.verify(envelope, start),
            Err(WebhookRejection::InvalidSignature)
        );
        envelope.signature = verifier
            .signature(signed_at, envelope.idempotency_key, payload)
            .leak();
        assert!(verifier.verify(envelope, start).is_ok());
    }
    #[test]
    fn unsigned_webhooks_are_rejected() {
        let start = now();
        let verifier = WebhookVerifier::new(b"secret".to_vec(), Duration::from_secs(60));
        let envelope = WebhookEnvelope {
            payload: b"{}",
            signature: "",
            idempotency_key: "unsigned",
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
            idempotency_key: "replay",
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
    fn webhook_outside_replay_window_is_rejected() {
        let start = now();
        let verifier = WebhookVerifier::new(b"secret".to_vec(), Duration::from_secs(60));
        let payload = b"{}";
        let signed_at = start - Duration::from_secs(120);
        let envelope = WebhookEnvelope {
            payload,
            signature: verifier.signature(signed_at, "old", payload).leak(),
            idempotency_key: "old",
            signed_at,
        };
        assert_eq!(
            verifier.verify(envelope, start),
            Err(WebhookRejection::OutsideReplayWindow)
        );
    }
    #[test]
    fn stale_success_cannot_close_new_half_open_probe() {
        let start = now();
        let runtime = ConnectorRuntime::new(
            RateLimitConfig {
                capacity: 10,
                refill_after: Duration::from_secs(1),
            },
            CircuitBreakerConfig {
                failure_threshold: 2,
                open_for: Duration::from_secs(30),
                failure_window: Duration::from_secs(60),
            },
            start,
        );
        let old = runtime.try_acquire_with_generation("gmail", start).unwrap();
        let f1 = runtime.try_acquire_with_generation("gmail", start).unwrap();
        let f2 = runtime.try_acquire_with_generation("gmail", start).unwrap();
        runtime.record_outcome(f1, false, start);
        runtime.record_outcome(f2, false, start);
        let probe = runtime
            .try_acquire_with_generation("gmail", start + Duration::from_secs(31))
            .unwrap();
        assert_ne!(old.epoch(), probe.epoch());
        runtime.record_outcome(old, true, start + Duration::from_secs(31));
        assert_eq!(runtime.breaker_state(), BreakerState::HalfOpen);
    }
    #[test]
    fn interleaved_successes_do_not_mask_windowed_failures() {
        // A repeatedly failing operation (e.g. timed-out writes) must trip
        // the breaker even when unrelated calls on the same connector keep
        // succeeding (e.g. preflight reads).
        let start = now();
        let runtime = ConnectorRuntime::new(
            RateLimitConfig::default(),
            CircuitBreakerConfig {
                failure_threshold: 3,
                open_for: Duration::from_secs(30),
                failure_window: Duration::from_secs(60),
            },
            start,
        );
        for i in 0..3u64 {
            let t = start + Duration::from_secs(i);
            let ok = runtime.try_acquire_with_generation("gmail", t).unwrap();
            runtime.record_outcome(ok, true, t);
            let bad = runtime.try_acquire_with_generation("gmail", t).unwrap();
            runtime.record_outcome(bad, false, t);
            if i < 2 {
                assert_eq!(runtime.breaker_state(), BreakerState::Closed);
            }
        }
        assert!(matches!(runtime.breaker_state(), BreakerState::Open { .. }));
    }
    #[test]
    fn failures_outside_window_expire() {
        let start = now();
        let runtime = ConnectorRuntime::new(
            RateLimitConfig::default(),
            CircuitBreakerConfig {
                failure_threshold: 2,
                open_for: Duration::from_secs(30),
                failure_window: Duration::from_secs(60),
            },
            start,
        );
        let f1 = runtime.try_acquire_with_generation("gmail", start).unwrap();
        runtime.record_outcome(f1, false, start);
        // Second failure lands 61s later: the first has expired, so the
        // breaker stays Closed.
        let late = start + Duration::from_secs(61);
        let f2 = runtime.try_acquire_with_generation("gmail", late).unwrap();
        runtime.record_outcome(f2, false, late);
        assert_eq!(runtime.breaker_state(), BreakerState::Closed);
    }
    #[test]
    fn timeout_outcomes_count_as_breaker_failures() {
        let start = now();
        let runtime = ConnectorRuntime::new(
            RateLimitConfig::default(),
            CircuitBreakerConfig {
                failure_threshold: 2,
                open_for: Duration::from_secs(30),
                failure_window: Duration::from_secs(60),
            },
            start,
        );
        let g = runtime.try_acquire_with_generation("gmail", start).unwrap();
        runtime.record_outcome(g, false, start);
        assert_eq!(runtime.breaker_state(), BreakerState::Closed);
        let g = runtime.try_acquire_with_generation("gmail", start).unwrap();
        runtime.record_outcome(g, false, start);
        assert!(matches!(runtime.breaker_state(), BreakerState::Open { .. }));
    }
    #[test]
    fn dropped_permit_reopens_breaker_instead_of_wedging_half_open() {
        let start = now();
        let runtime = ConnectorRuntime::new(
            RateLimitConfig::default(),
            CircuitBreakerConfig {
                failure_threshold: 1,
                open_for: Duration::from_secs(10),
                failure_window: Duration::from_secs(60),
            },
            start,
        );
        // Open the breaker.
        runtime.record_failure(start);
        assert!(matches!(runtime.breaker_state(), BreakerState::Open { .. }));
        // After cooldown, admit a probe (transitions to HalfOpen).
        let permit = runtime
            .try_acquire("gmail", start + Duration::from_secs(10))
            .expect("probe should be admitted after cooldown");
        assert_eq!(runtime.breaker_state(), BreakerState::HalfOpen);
        // Drop the permit without recording an outcome — simulates a cancelled
        // future. The drop handler reopens using Timestamp::now().
        drop(permit);
        assert!(matches!(runtime.breaker_state(), BreakerState::Open { .. }));
        // After cooldown (using wall-clock time since drop uses Timestamp::now()),
        // a new probe should be admitted.
        let cooldown_end = Timestamp::now() + Duration::from_secs(10);
        let _permit2 = runtime
            .try_acquire("gmail", cooldown_end)
            .expect("new probe should be admitted after re-cooldown");
    }
    use crate::connector_reality::{WebhookEnvelope, WebhookRejection, WebhookVerifier};
    use std::time::Duration;
    /// Owned buffers behind a borrowing `WebhookEnvelope` (no leak).
    struct OwnedWebhook {
        payload: Vec<u8>,
        signature: String,
        idempotency_key: String,
    }
    impl OwnedWebhook {
        fn envelope(&self, signed_at: Timestamp) -> WebhookEnvelope<'_> {
            WebhookEnvelope {
                payload: &self.payload,
                signature: &self.signature,
                idempotency_key: &self.idempotency_key,
                signed_at,
            }
        }
    }
    #[test]
    fn replay_cache_is_namespaced_per_route_and_bounds_key_length() {
        let now = now();
        let verifier = WebhookVerifier::new(b"test-secret", Duration::from_secs(300));
        let sat = now - Duration::from_secs(1);
        // Same idempotency key on two distinct routes verifies once each;
        // no cross-route replay collision.
        let sig_a = verifier.signature_bound(
            sat,
            "shared-key",
            "account-a",
            "openspine.status.read",
            b"{}",
        );
        let a = OwnedWebhook {
            payload: b"{}".to_vec(),
            signature: sig_a,
            idempotency_key: "shared-key".to_string(),
        };
        assert!(verifier
            .verify_bound(a.envelope(sat), "openspine.status.read", "account-a", now)
            .is_ok());
        // Replay on the SAME route is rejected.
        let sig_a2 = verifier.signature_bound(
            sat,
            "shared-key",
            "account-a",
            "openspine.status.read",
            b"{}",
        );
        let a2 = OwnedWebhook {
            payload: b"{}".to_vec(),
            signature: sig_a2,
            idempotency_key: "shared-key".to_string(),
        };
        assert!(matches!(
            verifier.verify_bound(a2.envelope(sat), "openspine.status.read", "account-a", now,),
            Err(WebhookRejection::Replayed)
        ));
        // The other route's identical key is still fresh.
        let sig_b = verifier.signature_bound(
            sat,
            "shared-key",
            "account-b",
            "openspine.status.read",
            b"{}",
        );
        let b = OwnedWebhook {
            payload: b"{}".to_vec(),
            signature: sig_b,
            idempotency_key: "shared-key".to_string(),
        };
        assert!(verifier
            .verify_bound(b.envelope(sat), "openspine.status.read", "account-b", now)
            .is_ok());
        let big = "x".repeat(4096);
        let sig_big =
            verifier.signature_bound(sat, &big, "account-a", "openspine.status.read", b"{}");
        let big_w = OwnedWebhook {
            payload: b"{}".to_vec(),
            signature: sig_big,
            idempotency_key: big,
        };
        assert!(matches!(
            verifier.verify_bound(
                big_w.envelope(sat),
                "openspine.status.read",
                "account-a",
                now,
            ),
            Err(WebhookRejection::KeyTooLong)
        ));
    }
    #[test]
    fn replay_cache_evicts_oldest_entries_at_capacity() {
        let now = now();
        let verifier = WebhookVerifier::new(b"test-secret", Duration::from_secs(300));
        // Monotonically increasing signed_at (all inside the 300s window)
        // so eviction deterministically removes the oldest entry.
        for i in 0..4097 {
            let sat = now - Duration::from_millis((4097 - i) as u64);
            let key = format!("key-{i}");
            let sig = verifier.signature_bound(sat, &key, "acct", "openspine.status.read", b"{}");
            let w = OwnedWebhook {
                payload: b"{}".to_vec(),
                signature: sig,
                idempotency_key: key,
            };
            assert!(verifier
                .verify_bound(w.envelope(sat), "openspine.status.read", "acct", now)
                .is_ok());
        }
        // Oldest entry (key-0, smallest signed_at) was evicted and its
        // replay is now accepted again.
        let sat0 = now - Duration::from_millis(4097u64);
        let sig0 = verifier.signature_bound(sat0, "key-0", "acct", "openspine.status.read", b"{}");
        let oldest = OwnedWebhook {
            payload: b"{}".to_vec(),
            signature: sig0,
            idempotency_key: "key-0".to_string(),
        };
        assert!(verifier
            .verify_bound(oldest.envelope(sat0), "openspine.status.read", "acct", now)
            .is_ok());
        let sat_fresh = now - Duration::from_millis(1u64);
        let sigf = verifier.signature_bound(
            sat_fresh,
            "key-fresh",
            "acct",
            "openspine.status.read",
            b"{}",
        );
        let fresh = OwnedWebhook {
            payload: b"{}".to_vec(),
            signature: sigf,
            idempotency_key: "key-fresh".to_string(),
        };
        assert!(verifier
            .verify_bound(
                fresh.envelope(sat_fresh),
                "openspine.status.read",
                "acct",
                now
            )
            .is_ok());
    }
}
