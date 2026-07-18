pub(crate) fn handoff_complete(outcome: crate::pipeline::NotifyOutcome) -> bool {
    matches!(
        outcome,
        crate::pipeline::NotifyOutcome::Sent
            | crate::pipeline::NotifyOutcome::OutcomeAuditFailed
            | crate::pipeline::NotifyOutcome::SendFailed
    )
}

use std::sync::Arc;

/// Replay pending kernel-owned nerve deliveries through the governed owner
/// notification path. Delivery rows remain pending until the notification is
/// durably handed off or its terminal outcome is audited.
pub(crate) async fn run(state: Arc<crate::pipeline::AppState>) {
    loop {
        match state.store.pending_nerve_deliveries() {
            Ok(items) => {
                for (interjection_id, class_digest) in items {
                    let outcome = crate::pipeline::notify_owner_with_digest(
                        &state,
                        state.owner_user_id,
                        &format!("A governed screener notice is ready (digest {class_digest})."),
                        &[],
                        None,
                    )
                    .await;
                    if handoff_complete(outcome) {
                        if let Err(err) = state.store.ack_nerve_delivery(&interjection_id) {
                            tracing::error!("acknowledging nerve delivery: {err}");
                        }
                    } else {
                        tracing::warn!(
                            ?outcome,
                            "nerve delivery not durably handed off; retaining for retry"
                        );
                    }
                }
            }
            Err(err) => tracing::error!("loading nerve deliveries: {err}"),
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}
