use super::*;

#[test]
fn ack_policy_is_exhaustive_and_deduplicating() {
    let durable = [
        pipeline::NotifyOutcome::Sent,
        pipeline::NotifyOutcome::OutcomeAuditFailed,
        pipeline::NotifyOutcome::SendFailed,
    ];
    let retained = [
        pipeline::NotifyOutcome::GateUnavailable,
        pipeline::NotifyOutcome::GateAuditFailed,
        pipeline::NotifyOutcome::GateDenied,
        pipeline::NotifyOutcome::AttemptAuditFailed,
        pipeline::NotifyOutcome::DeadLetterPersistFailed,
    ];
    assert!(durable.into_iter().all(nerve_delivery::handoff_complete));
    assert!(retained
        .into_iter()
        .all(|outcome| !nerve_delivery::handoff_complete(outcome)));
}
