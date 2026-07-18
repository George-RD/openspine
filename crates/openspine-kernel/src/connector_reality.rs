// openspine:allow-large-module reason: connector_reality is the single kernel-owned module for all connector admission primitives (rate limit, circuit breaker, webhook verification). Splitting would scatter the state machines that every connector lane must use, increasing the risk of inconsistent admission semantics across lanes.
//! Kernel-owned connector admission and authenticity primitives (AD-141/AD-103).
//!
//! This module deliberately has no connector consumers yet. It provides the
//! state machines that every future connector lane must use before touching an
//! external service: bounded token buckets, circuit breakers, and signed,
//! replay-protected webhook envelopes.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use hmac_sha256::HMAC;
use jiff::Timestamp;
use parking_lot::Mutex;

/// BoringSSL-style `sha256=` prefix used on the wire (GitHub/webhook convention).
#[allow(dead_code)] // webhook substrate; no consumer exists yet (AD-141)
const SIGNATURE_PREFIX: &str = "sha256=";

pub const CONNECTOR_UNAVAILABLE_AUDIT_KIND: &str = "connector_unavailable";

/// Admission policy for one connector's request bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitConfig {
    pub capacity: u32,
    pub refill_after: Duration,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            capacity: 10,
            refill_after: Duration::from_millis(100),
        }
    }
}

/// A deterministic token bucket. Callers supply the clock so tests and
/// recovery code do not depend on wall-clock sleeps.
#[derive(Debug, Clone)]
pub struct RateLimitBucket {
    config: RateLimitConfig,
    available: u32,
    last_refill: Timestamp,
}

impl RateLimitBucket {
    pub fn new(config: RateLimitConfig, now: Timestamp) -> Self {
        let capacity = config.capacity.max(1);
        Self {
            config: RateLimitConfig { capacity, ..config },
            available: capacity,
            last_refill: now,
        }
    }

    /// Cheap peek: would a call at `now` have capacity? Does not mutate.
    fn has_capacity(&self, now: Timestamp) -> bool {
        if self.available > 0 {
            return true;
        }
        // Empty — only refilled if enough time elapsed to issue >=1 permit.
        if now <= self.last_refill {
            return false;
        }
        let elapsed = now
            .since(self.last_refill)
            .ok()
            .and_then(|duration| Duration::try_from(duration).ok())
            .unwrap_or_default();
        elapsed >= self.config.refill_after
    }

    pub fn try_acquire(&mut self, now: Timestamp) -> Result<(), Duration> {
        self.refill(now);
        if self.available > 0 {
            self.available -= 1;
            return Ok(());
        }
        Err(self.retry_after(now))
    }

    fn refill(&mut self, now: Timestamp) {
        if now <= self.last_refill {
            return;
        }
        let elapsed = now
            .since(self.last_refill)
            .ok()
            .and_then(|duration| Duration::try_from(duration).ok())
            .unwrap_or_default();
        let interval_nanos = self.config.refill_after.as_nanos().max(1);
        let permits =
            (elapsed.as_nanos() / interval_nanos).min(u128::from(self.config.capacity)) as u32;
        if permits > 0 {
            self.available = self
                .available
                .saturating_add(permits)
                .min(self.config.capacity);
            // Once the bucket is full, discard all idle elapsed time. This
            // prevents a long idle period from creating a burst larger than the
            // configured capacity on the next call.
            if self.available == self.config.capacity {
                self.last_refill = now;
            } else {
                self.last_refill += self.config.refill_after.mul_f64(permits as f64);
            }
        }
    }

    fn retry_after(&self, now: Timestamp) -> Duration {
        let elapsed = now
            .since(self.last_refill)
            .ok()
            .and_then(|duration| Duration::try_from(duration).ok())
            .unwrap_or_default();
        self.config
            .refill_after
            .saturating_sub(elapsed)
            .max(Duration::from_nanos(1))
    }
}

/// Breaker policy for one connector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub open_for: Duration,
    /// Failures within this window count toward the threshold. Interleaved
    /// successes close a HalfOpen probe but never erase recorded failures, so
    /// a repeatedly failing operation (e.g. timed-out writes) cannot be masked
    /// by unrelated successes (e.g. preflight reads) on the same connector.
    pub failure_window: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 3,
            open_for: Duration::from_secs(30),
            failure_window: Duration::from_secs(60),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerState {
    Closed,
    Open { until: Timestamp },
    HalfOpen,
}

#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    state: BreakerState,
    /// Timestamps of failures inside the sliding `failure_window`.
    recent_failures: std::collections::VecDeque<Timestamp>,
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config: CircuitBreakerConfig {
                failure_threshold: config.failure_threshold.max(1),
                ..config
            },
            state: BreakerState::Closed,
            recent_failures: std::collections::VecDeque::new(),
        }
    }

    pub fn state(&self) -> BreakerState {
        self.state
    }

    /// Returns `Ok(())` if a single probe may proceed. On cooldown expiry the
    /// Closed→Open→HalfOpen transition is committed here; callers MUST record
    /// an outcome (success or failure) before the next call.
    pub fn allow(&mut self, now: Timestamp) -> Result<(), BreakerState> {
        match self.state {
            BreakerState::Closed => Ok(()),
            BreakerState::Open { until } if now < until => Err(self.state),
            BreakerState::Open { .. } => {
                self.state = BreakerState::HalfOpen;
                Ok(())
            }
            BreakerState::HalfOpen => Err(self.state),
        }
    }

    pub fn record_success(&mut self) {
        // A success closes the breaker (HalfOpen probe passed / Closed stays
        // Closed) but deliberately leaves the failure window intact: unrelated
        // successes must not launder a failing operation below the threshold.
        self.state = BreakerState::Closed;
    }

    pub fn record_failure(&mut self, now: Timestamp) {
        match self.state {
            BreakerState::HalfOpen => self.open(now),
            BreakerState::Closed => {
                self.prune_failures(now);
                self.recent_failures.push_back(now);
                if self.recent_failures.len() as u32 >= self.config.failure_threshold {
                    self.open(now);
                }
            }
            BreakerState::Open { .. } => {}
        }
    }

    fn prune_failures(&mut self, now: Timestamp) {
        while let Some(&oldest) = self.recent_failures.front() {
            if now
                .since(oldest)
                .ok()
                .and_then(|d| Duration::try_from(d).ok())
                .is_some_and(|d| d > self.config.failure_window)
            {
                self.recent_failures.pop_front();
            } else {
                break;
            }
        }
    }

    fn open(&mut self, now: Timestamp) {
        self.state = BreakerState::Open {
            until: now + self.config.open_for,
        };
        self.recent_failures.clear();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConnectorCallError {
    #[error("connector {connector} rate limited; retry after {retry_after:?}")]
    RateLimited {
        connector: String,
        retry_after: Duration,
    },
    /// Admission rejected by a genuinely Open/HalfOpen breaker. This is the
    /// distinct `connector_unavailable` condition, separate from a policy
    /// denial or an ordinary call failure.
    #[error("connector {connector} unavailable ({state:?})")]
    Unavailable {
        connector: String,
        state: BreakerState,
    },
}

/// Shared admission state held by a connector registry and its probe permits.
#[derive(Debug, Clone)]
pub struct ConnectorRuntime {
    state: Arc<Mutex<ConnectorRuntimeState>>,
}

#[derive(Debug)]
struct ConnectorRuntimeState {
    bucket: RateLimitBucket,
    breaker: CircuitBreaker,
    epoch: u64,
}

/// An admitted connector call whose outcome must be recorded.
///
/// A permit admitted while the breaker is half-open owns that probe. If the
/// future is cancelled or otherwise dropped before recording an outcome, its
/// drop handler reopens the breaker instead of leaving it wedged half-open.
#[derive(Debug)]
pub struct ConnectorProbePermit {
    state: Arc<Mutex<ConnectorRuntimeState>>,
    epoch: u64,
    armed: bool,
}

impl ConnectorProbePermit {
    #[allow(dead_code)]
    /// Record the connector result and disarm this permit.
    pub fn record_outcome(self, succeeded: bool) {
        self.record_outcome_at(succeeded, Timestamp::now());
    }

    /// Deterministic-clock variant used by kernel tests.
    pub fn record_outcome_at(mut self, succeeded: bool, now: Timestamp) {
        self.armed = false;
        let mut state = self.state.lock();
        if state.epoch != self.epoch {
            return;
        }
        let was_open = matches!(state.breaker_state(), BreakerState::Open { .. });
        if succeeded {
            state.breaker.record_success();
        } else {
            state.breaker.record_failure(now);
            if !was_open && matches!(state.breaker_state(), BreakerState::Open { .. }) {
                state.epoch = state.epoch.wrapping_add(1);
            }
        }
    }
    #[allow(dead_code)]
    /// The epoch at which this permit was admitted. Used by tests to verify
    /// epoch advancement on breaker transitions.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
}

impl Drop for ConnectorProbePermit {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.armed = false;
        let mut state = self.state.lock();
        if state.epoch == self.epoch && matches!(state.breaker_state(), BreakerState::HalfOpen) {
            state.breaker.record_failure(Timestamp::now());
            state.epoch = state.epoch.wrapping_add(1);
        }
    }
}

impl ConnectorRuntimeState {
    fn breaker_state(&self) -> BreakerState {
        self.breaker.state()
    }
}

impl ConnectorRuntime {
    pub fn new(rate_limit: RateLimitConfig, breaker: CircuitBreakerConfig, now: Timestamp) -> Self {
        Self {
            state: Arc::new(Mutex::new(ConnectorRuntimeState {
                bucket: RateLimitBucket::new(rate_limit, now),
                breaker: CircuitBreaker::new(breaker),
                epoch: 0,
            })),
        }
    }

    pub fn try_acquire(
        &self,
        connector: &str,
        now: Timestamp,
    ) -> Result<ConnectorProbePermit, ConnectorCallError> {
        let mut state = self.state.lock();
        // An actively blocking breaker (Open until a future instant, or
        // HalfOpen pending a probe result) must win over rate limiting so the
        // distinct `connector_unavailable` outcome is reachable.
        match state.breaker_state() {
            BreakerState::Open { until } if now < until => {
                return Err(ConnectorCallError::Unavailable {
                    connector: connector.to_string(),
                    state: state.breaker_state(),
                });
            }
            BreakerState::HalfOpen => {
                return Err(ConnectorCallError::Unavailable {
                    connector: connector.to_string(),
                    state: state.breaker_state(),
                });
            }
            BreakerState::Closed | BreakerState::Open { .. } => {}
        }
        // Breaker is Closed, or a timed-out Open may admit one probe. Reserve
        // capacity before committing that probe so rate limiting cannot strand
        // the breaker in HalfOpen.
        if !state.bucket.has_capacity(now) {
            return Err(ConnectorCallError::RateLimited {
                connector: connector.to_string(),
                retry_after: state.bucket.retry_after(now),
            });
        }
        state
            .breaker
            .allow(now)
            .map_err(|state_value| ConnectorCallError::Unavailable {
                connector: connector.to_string(),
                state: state_value,
            })?;
        if state.bucket.try_acquire(now).is_err() {
            return Err(ConnectorCallError::RateLimited {
                connector: connector.to_string(),
                retry_after: state.bucket.retry_after(now),
            });
        }
        Ok(ConnectorProbePermit {
            state: Arc::clone(&self.state),
            epoch: state.epoch,
            armed: true,
        })
    }

    pub fn try_acquire_with_generation(
        &self,
        connector: &str,
        now: Timestamp,
    ) -> Result<ConnectorProbePermit, ConnectorCallError> {
        self.try_acquire(connector, now)
    }

    pub fn record_outcome(&self, permit: ConnectorProbePermit, succeeded: bool, now: Timestamp) {
        permit.record_outcome_at(succeeded, now);
    }

    #[allow(dead_code)]
    pub fn record_success(&self) {
        let mut state = self.state.lock();
        state.breaker.record_success();
    }

    pub fn record_failure(&self, now: Timestamp) {
        let mut state = self.state.lock();
        let was_open = matches!(state.breaker_state(), BreakerState::Open { .. });
        state.breaker.record_failure(now);
        if !was_open && matches!(state.breaker_state(), BreakerState::Open { .. }) {
            state.epoch = state.epoch.wrapping_add(1);
        }
    }

    #[allow(dead_code)] // introspection helper; tests + future dashboards use it
    pub fn breaker_state(&self) -> BreakerState {
        self.state.lock().breaker_state()
    }
}

/// A verified webhook request. The legacy signature covers `signed_at`, the
/// idempotency key, and payload; the headless lane uses `verify_bound` to
/// additionally authenticate its route selector.
#[allow(dead_code)] // shared webhook substrate and headless lane
#[derive(Debug, Clone, Copy)]
pub struct WebhookEnvelope<'a> {
    pub payload: &'a [u8],
    pub signature: &'a str,
    pub idempotency_key: &'a str,
    pub signed_at: Timestamp,
}

#[allow(dead_code)] // webhook substrate; no consumer exists yet (AD-141)
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WebhookRejection {
    #[error("webhook signature is missing")]
    MissingSignature,
    #[error("webhook signature is invalid")]
    InvalidSignature,
    #[error("webhook idempotency key is missing")]
    MissingIdempotencyKey,
    #[error("webhook timestamp is outside the replay window")]
    OutsideReplayWindow,
    #[error("webhook idempotency key was already consumed")]
    Replayed,
    #[error("webhook idempotency key exceeds the length cap")]
    KeyTooLong,
}

/// Maximum byte length of an idempotency key accepted by the verifier.
/// Keys beyond this are rejected before they can touch the replay cache
/// (a bound on the per-(route, key) cache key size).
const MAX_IDEMPOTENCY_KEY_LEN: usize = 2048;
/// in-window entries are evicted first so the cache cannot grow unbounded
/// across many distinct webhook routes.
const MAX_REPLAY_CACHE_ENTRIES: usize = 4096;
type ReplayKey = (String, String, String);

/// HMAC-SHA256 verifier with a bounded, one-process idempotency cache.
/// Headless consumers persist the verified event and use the bound-MAC
/// variant; durable cross-process idempotency remains a store concern.
#[allow(dead_code)] // shared webhook substrate and headless lane
#[derive(Clone)]
pub struct WebhookVerifier {
    secret: Arc<[u8]>,
    replay_window: Duration,
    seen: Arc<Mutex<HashMap<ReplayKey, Timestamp>>>,
}
impl std::fmt::Debug for WebhookVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebhookVerifier")
            .field("secret", &"<redacted>")
            .field("replay_window", &self.replay_window)
            .field("seen", &"<redacted>")
            .finish()
    }
}

#[allow(dead_code)] // webhook substrate; no consumer exists yet (AD-141)
impl WebhookVerifier {
    pub fn new(secret: impl Into<Vec<u8>>, replay_window: Duration) -> Self {
        Self {
            secret: Arc::from(secret.into()),
            replay_window: replay_window.max(Duration::from_secs(1)),
            seen: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    /// True when no HMAC secret was configured (an empty/forgeable key).
    /// Production ingress must refuse webhooks in this state (fail-closed).
    pub fn is_key_unset(&self) -> bool {
        self.secret.is_empty()
    }

    pub fn verify(
        &self,
        envelope: WebhookEnvelope<'_>,
        now: Timestamp,
    ) -> Result<(), WebhookRejection> {
        self.verify_with_message(
            envelope,
            now,
            "",
            "",
            signed_at_key_and_payload(
                envelope.payload,
                envelope.idempotency_key,
                envelope.signed_at,
            ),
        )
    }

    /// Verify a delivery whose authenticated route selector is included in
    /// the MAC preimage. Headless consumers must use this variant so a valid
    /// delivery cannot be retargeted to another registered hook route.
    pub fn verify_bound(
        &self,
        envelope: WebhookEnvelope<'_>,
        action: &str,
        channel_account: &str,
        now: Timestamp,
    ) -> Result<(), WebhookRejection> {
        self.verify_with_message(
            envelope,
            now,
            channel_account,
            action,
            signed_at_key_account_and_payload(
                envelope.payload,
                envelope.idempotency_key,
                channel_account,
                action,
                envelope.signed_at,
            ),
        )
    }

    fn verify_with_message(
        &self,
        envelope: WebhookEnvelope<'_>,
        now: Timestamp,
        channel_account: &str,
        action: &str,
        message: Vec<u8>,
    ) -> Result<(), WebhookRejection> {
        if envelope.signature.trim().is_empty() {
            return Err(WebhookRejection::MissingSignature);
        }
        if envelope.idempotency_key.trim().is_empty() {
            return Err(WebhookRejection::MissingIdempotencyKey);
        }
        if envelope.idempotency_key.len() > MAX_IDEMPOTENCY_KEY_LEN {
            return Err(WebhookRejection::KeyTooLong);
        }
        let age = now
            .since(envelope.signed_at)
            .ok()
            .and_then(|duration| Duration::try_from(duration).ok());
        if age.is_none() || age.is_some_and(|value| value > self.replay_window) {
            return Err(WebhookRejection::OutsideReplayWindow);
        }
        let supplied = envelope
            .signature
            .strip_prefix(SIGNATURE_PREFIX)
            .unwrap_or(envelope.signature);
        let supplied_bytes =
            decode_hex(supplied.as_bytes()).ok_or(WebhookRejection::InvalidSignature)?;
        if !HMAC::verify(&message, &*self.secret, &supplied_bytes) {
            return Err(WebhookRejection::InvalidSignature);
        }
        // Replay cache is namespaced per (route, action) so a valid delivery
        // cannot be replay-protected against a different route's key space.
        let cache_key = (
            channel_account.to_string(),
            envelope.idempotency_key.to_string(),
            action.to_string(),
        );
        let mut seen = self.seen.lock();
        seen.retain(|_, timestamp| {
            now.since(*timestamp)
                .ok()
                .and_then(|duration| Duration::try_from(duration).ok())
                .is_some_and(|age| age <= self.replay_window)
        });
        // Replay check BEFORE eviction: a replay of the oldest entry at
        // full capacity must be detected, never evicted-then-accepted.
        if seen.contains_key(&cache_key) {
            return Err(WebhookRejection::Replayed);
        }
        // Capacity bound: evict the oldest in-window entries first, then
        // insert the new key (eviction can never remove the key we just
        // confirmed is absent).
        if seen.len() >= MAX_REPLAY_CACHE_ENTRIES {
            let overflow = seen.len() - MAX_REPLAY_CACHE_ENTRIES + 1;
            let mut entries: Vec<(ReplayKey, Timestamp)> =
                seen.iter().map(|(k, v)| (k.clone(), *v)).collect();
            entries.sort_by_key(|(_, v)| *v);
            for (k, _) in entries.into_iter().take(overflow) {
                seen.remove(&k);
            }
        }
        seen.insert(cache_key, envelope.signed_at);
        Ok(())
    }

    /// Compute the HMAC-SHA256 tag over a length-delimited
    /// `signed_at.idempotency_key.payload` pre-image. Binding the key into the
    /// MAC means a captured `(payload, signature)` cannot be replayed under a
    /// different idempotency key.
    pub fn signature_bytes(
        &self,
        signed_at: Timestamp,
        idempotency_key: &str,
        payload: &[u8],
    ) -> [u8; 32] {
        HMAC::mac(
            signed_at_key_and_payload(payload, idempotency_key, signed_at),
            &*self.secret,
        )
    }

    /// Convenience hex encoding for wire tests / tooling.
    pub fn signature(&self, signed_at: Timestamp, idempotency_key: &str, payload: &[u8]) -> String {
        let tag = self.signature_bytes(signed_at, idempotency_key, payload);
        let mut hex = String::with_capacity(SIGNATURE_PREFIX.len() + 64);
        hex.push_str(SIGNATURE_PREFIX);
        for byte in tag {
            hex.push_str(&format!("{byte:02x}"));
        }
        hex
    }
    /// Compute a signature with the route selector and action bound into the
    /// preimage, so a captured valid delivery cannot be retargeted to another
    /// grant-allowed action.
    pub fn signature_bound(
        &self,
        signed_at: Timestamp,
        idempotency_key: &str,
        channel_account: &str,
        action: &str,
        payload: &[u8],
    ) -> String {
        let tag = HMAC::mac(
            signed_at_key_account_and_payload(
                payload,
                idempotency_key,
                channel_account,
                action,
                signed_at,
            ),
            &*self.secret,
        );
        let mut hex = String::with_capacity(SIGNATURE_PREFIX.len() + 64);
        hex.push_str(SIGNATURE_PREFIX);
        for byte in tag {
            hex.push_str(&format!("{byte:02x}"));
        }
        hex
    }
}

/// Build the length-delimited MAC pre-image:
/// `signed_at "." len(key) key payload`. The length prefix removes any
/// ambiguity between `key="ab" payload="cd"` and `key="abc" payload="d"`.
fn signed_at_key_and_payload(
    payload: &[u8],
    idempotency_key: &str,
    signed_at: Timestamp,
) -> Vec<u8> {
    let timestamp = signed_at.to_string();
    let key = idempotency_key.as_bytes();
    let mut message = Vec::with_capacity(
        timestamp.len() + 1 + std::mem::size_of::<u64>() + key.len() + payload.len(),
    );
    message.extend_from_slice(timestamp.as_bytes());
    message.push(b'.');
    message.extend_from_slice(&(key.len() as u64).to_be_bytes());
    message.extend_from_slice(key);
    message.extend_from_slice(payload);
    message
}
/// Build the route-bound MAC preimage binding the channel account AND the
/// action. The length prefixes prevent concatenation ambiguity between
/// route selector, action, and payload bytes, so a signed delivery can
/// neither be retargeted to another route nor to another action.
fn signed_at_key_account_and_payload(
    payload: &[u8],
    idempotency_key: &str,
    channel_account: &str,
    action: &str,
    signed_at: Timestamp,
) -> Vec<u8> {
    let mut message = signed_at_key_and_payload(&[], idempotency_key, signed_at);
    let account = channel_account.as_bytes();
    message.extend_from_slice(&(account.len() as u64).to_be_bytes());
    message.extend_from_slice(account);
    let action_bytes = action.as_bytes();
    message.extend_from_slice(&(action_bytes.len() as u64).to_be_bytes());
    message.extend_from_slice(action_bytes);
    message.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    message.extend_from_slice(payload);
    message
}

fn decode_hex(input: &[u8]) -> Option<[u8; 32]> {
    if input.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (slot, pair) in out.iter_mut().zip(input.chunks_exact(2)) {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        *slot = ((hi << 4) | lo) as u8;
    }
    Some(out)
}

impl fmt::Display for BreakerState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => formatter.write_str("closed"),
            Self::Open { .. } => formatter.write_str("open"),
            Self::HalfOpen => formatter.write_str("half_open"),
        }
    }
}

#[cfg(test)]
mod connector_reality_tests;
