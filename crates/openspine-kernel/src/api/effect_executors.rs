//! Kernel-owned effect-executor registry (#127).
//!
//! Every catalogued action whose D-146 `ActionImplementationDescriptor`
//! names an `executor_id` resolves that id to exactly one kernel-owned
//! executor here. The registry is the second, independent readiness axis: a
//! descriptor alone never proves a live effect path exists
//! (`AppState::is_execution_backed` requires both).
//!
//! Deliberately absent: any fallback. A lookup miss is not a stub and not a
//! default executor — the dispatcher fails closed with
//! `DispatchError::NoExecutor`, and an approved lane with no registered
//! executor performs no effect. Only `email.create_draft` has one today;
//! `email.send` and every other unwired effect id resolve to nothing.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use openspine_schemas::action::ActionRequest;
use openspine_schemas::grant::TaskGrant;

use crate::pipeline::AppState;

/// The boxed future returned by one effect executor.
pub(crate) type EffectExecutorFuture<'a> =
    Pin<Box<dyn Future<Output = anyhow::Result<EffectOutcome>> + Send + 'a>>;

/// A kernel-owned executor for one catalogued effect implementation.
pub(crate) type EffectExecutor =
    for<'a> fn(&'a AppState, &'a TaskGrant, &'a ActionRequest, i64) -> EffectExecutorFuture<'a>;

/// The truthful result of one attempted effect. The safety-relevant partition
/// is "may an external write have reached the provider": only
/// [`Self::Executed`] and [`Self::DeliveryUnknown`] answer yes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EffectOutcome {
    /// The provider confirmed the write, and its durable evidence — audit row
    /// plus a resolved pending-write fence — is recorded.
    Executed,
    /// The executor refused before polling any *write* future: no write permit
    /// was consumed, this attempt recorded no pending-write fence, and no
    /// provider write was attempted. Covers every pre-effect re-derivation
    /// refusal and a rejected write admission. Note the executor may already
    /// have performed its read-only recipient re-derivation (a Gmail thread
    /// fetch) before refusing — that read is not an effect, but "nothing was
    /// sent" is the precise claim here, not "nothing was called".
    RefusedPreEffect,
    /// The write future was polled and its result is ambiguous — the provider
    /// may have acted before the response was lost. The pending-write fence is
    /// left OPEN and the write is never automatically retried, because Gmail's
    /// `create_draft` has no idempotency key. Deliberately conservative: an
    /// ambiguous failure anywhere inside the polled future lands here rather
    /// than being guessed as a confirmed failure.
    DeliveryUnknown,
    /// The write future was polled and returned a *definite* failure, so no
    /// effect took hold; the pending-write fence has been resolved. Reached
    /// only after admission succeeded — a rejected permit is
    /// [`Self::RefusedPreEffect`] instead.
    FailedAfterAttempt,
}

/// Kernel-owned effect executors keyed by their catalog `executor_id`.
pub(crate) struct EffectExecutorRegistry {
    map: HashMap<&'static str, EffectExecutor>,
}

impl EffectExecutorRegistry {
    /// The one-to-one mapping of catalogued `executor_id`s to kernel
    /// executors. Adding an id here is what makes the action
    /// execution-backed; removing it makes every admission source fail
    /// closed rather than silently no-op.
    pub(crate) fn default_registrations() -> Self {
        let mut map: HashMap<&'static str, EffectExecutor> = HashMap::new();
        map.insert(
            "gmail.create_draft",
            crate::pipeline::gmail_create_draft_executor as EffectExecutor,
        );
        EffectExecutorRegistry { map }
    }

    /// A registry with no executors, for proving that an admission source
    /// holding a standing-rule reservation still fails closed — and releases
    /// that reservation — when its catalogued `executor_id` resolves to
    /// nothing.
    #[cfg(test)]
    pub(crate) fn empty() -> Self {
        EffectExecutorRegistry {
            map: HashMap::new(),
        }
    }

    /// Resolve an executor by stable executor id.
    pub(crate) fn lookup(&self, executor_id: &str) -> Option<EffectExecutor> {
        self.map.get(executor_id).copied()
    }
}
