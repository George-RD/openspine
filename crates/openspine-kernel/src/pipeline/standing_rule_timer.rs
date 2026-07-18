// openspine:allow-large-module reason: dark-window consumer (claim/redispatch/recovery) regression tests keep this file past the 500-line gate.
//! Dark-window consumer for standing rules (AD-012 leaning).
//!
//! Rides the same `workflow.timer_fired` ledger event + D-074 kernel timer
//! driver as `run_task_deadline_consumer`, but correlates fired timers back
//! to a standing rule via `standing_rule_timer_links` rather than a task. On a
//! fired timer whose default is `Allow`, `claim_standing_rule_dark_window`
//! durably decides the pre-agreed default and returns the pending action; the
//! consumer then re-dispatches it through the shared mediation boundary
//! (`mediate_and_dispatch_action`) carrying the digest-bound, one-use fired
//! token. `Deny` resolves `denied` and dispatches nothing. The claim and the
//! token consumption are each transactionally idempotent (D-082 precedent) so
//! a replayed `workflow.timer_fired` event never double-applies a default or
//! double-grants an effect.
//!
//! AD-104/AD-012: the kernel-owned dark-window timer driver. Consumers only
//! schedule (`WorkflowCtx::schedule_timer`) and subscribe (poll or ledger
//! replay of `workflow.timer_fired`); this loop is what actually fires due
//! timers, sleeping until the earliest known deadline.

use jiff::Timestamp;
use openspine_schemas::action::GateDecision;
use openspine_schemas::artifact::ArtifactRef;
use openspine_schemas::audit::AuditKind;
use openspine_schemas::event_bus::{ConsumerCheckpoint, EventSubscriptionFilter};
use serde_json::Value;

use super::AppState;
use crate::api::actions::mediate_and_dispatch_action;
use crate::store::event_bus::PersistedConsumerState;
use crate::store::StoreError;

/// Run forever: replay every `workflow.timer_fired` event, and for any that
/// belongs to a standing dark window, claim its pending action and re-dispatch
/// it through the mediation boundary (or apply the `Deny` default). Uses an
/// independent `consumer_id` so it shares the event stream with the task
/// deadline consumer without sharing a checkpoint.
pub(crate) async fn run_standing_rule_dark_window_consumer(state: &AppState) -> ! {
    let filter = EventSubscriptionFilter::kinds([
        AuditKind::new("workflow.timer_fired").expect("static audit kind is valid")
    ]);
    let consumer_id = "standing_rule_dark_window_consumer";
    let mut checkpoint = match state.store.load_consumer_checkpoint(consumer_id) {
        Ok(Some(saved)) => saved.checkpoint,
        Ok(None) => ConsumerCheckpoint::default(),
        Err(err) => {
            tracing::error!(error = %err, "standing-rule dark-window checkpoint load failed; starting fresh");
            ConsumerCheckpoint::default()
        }
    };
    loop {
        // Crash-recovery sweep: two receiptless states.
        // 1. `none`: the one-use token was never consumed (crash between claim
        //    and dispatch) — re-drive exactly once. Decode errors propagate so
        //    a corrupt/missing payload is never silently skipped.
        // 2. `claimed`: the token was consumed but the connector effect was
        //    never durably attempted — surface for owner attention (fail
        //    closed) and never re-run the effect.
        if let Err(err) = recover_unredriven_pending(state).await {
            tracing::error!(error = %err, "standing-rule dark-window recovery sweep failed");
        }
        match state
            .store
            .replay_audit(&filter, checkpoint.last_acked_global_seq)
        {
            Ok(entries) => {
                for entry in entries {
                    // `claim_standing_rule_dark_window` returns `Ok(Some(..))`
                    // only when this timer fresh-applies a standing rule's
                    // `Allow` default; non-standing-rule timers and
                    // already-claimed timers yield `Ok(None)` and are safely
                    // acknowledged. There is no transient retry path here: a
                    // default is either applied (idempotently) or N/A, and a
                    // store error is logged and the event withheld by breaking
                    // the loop.
                    let timer_id = standing_rule_timer_id_from_event(&entry.event);
                    let Some(timer_id) = timer_id else {
                        // Not a timer we own (e.g. a task deadline): skip
                        // without acknowledgement break. The task consumer
                        // handles its own dispatch state.
                        checkpoint.last_acked_global_seq = entry.global_seq;
                        continue;
                    };
                    match claim_and_redispatch(state, &timer_id, entry.event.ts).await {
                        Ok(()) => {}
                        Err(err) => {
                            tracing::error!(
                                error = %err,
                                global_seq = entry.global_seq,
                                "standing-rule dark-window apply failed; retrying"
                            );
                            break;
                        }
                    }
                    checkpoint.last_acked_global_seq = entry.global_seq;
                    if let Err(err) = state.store.save_consumer_checkpoint(
                        consumer_id,
                        &PersistedConsumerState {
                            schema_version: 1,
                            checkpoint: checkpoint.clone(),
                            filter: filter.clone(),
                        },
                    ) {
                        tracing::error!(error = %err, "standing-rule dark-window checkpoint save failed");
                    }
                }
            }
            Err(err) => tracing::error!(error = %err, "standing-rule dark-window replay failed"),
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

/// Decode a pending action's encrypted `payload_ref` back to JSON. Errors are
/// propagated (never swallowed) so a payload-bearing fired default is never
/// re-dispatched with `None` payload — which would mismatch the digest-bound
/// token fingerprint and fail closed (P1-7/P1-11).
fn decode_pending_payload(
    state: &AppState,
    payload_ref: &Option<ArtifactRef>,
) -> Result<Option<Value>, StoreError> {
    match payload_ref {
        Some(r) => {
            let bytes = state.artifacts.get(r).map_err(StoreError::ArtifactStore)?;
            Ok(Some(
                serde_json::from_slice(&bytes).map_err(StoreError::Serde)?,
            ))
        }
        None => Ok(None),
    }
}

/// Claim a fired dark-window timer and re-dispatch its pending action through
/// the shared mediation boundary. Returns `Ok(())` when the timer was not
/// ours, was already claimed, resolved `Deny`, or was successfully re-dispatched
/// (or the re-dispatch failed and the reservation was cancelled). On `Err` the
/// caller should withhold acknowledgement and retry.
async fn claim_and_redispatch(
    state: &AppState,
    timer_id: &str,
    now: jiff::Timestamp,
) -> Result<(), StoreError> {
    let Some(pending) = state.store.claim_standing_rule_dark_window(timer_id, now)? else {
        return Ok(());
    };
    // `claim` returns `None` for Deny/terminal timers (nothing to dispatch).
    if pending.resolution.as_deref() != Some("allowed") {
        return Ok(());
    }
    let Some(grant) = state.store.find_task_grant_by_id(pending.task_grant_id)? else {
        tracing::error!(pending_id = %pending.pending_id, "standing-rule dark-window grant missing; cannot redispatch");
        return Ok(());
    };
    // Reconstruct the original encrypted payload so the re-dispatch carries the
    // exact bytes the fingerprint was computed over (P1-7/P1-11 token match).
    let payload_json = decode_pending_payload(state, &pending.payload_ref)?;
    let (decision, _, result, _) = mediate_and_dispatch_action(
        state,
        &grant.0,
        pending.action_id.clone(),
        pending.bound_chat_id,
        payload_json.as_ref(),
        crate::api::actions::FailureSurface::Detached,
        Some(&pending.pending_id),
    )
    .await
    .map_err(|e| StoreError::FailureRouting(format!("dark-window redispatch failed: {e:?}")))?;
    // Fail closed: only report success when the mediation boundary actually
    // allowed and produced an effect (the one-use token was consumed and the
    // fired-reservation waiver finalized). Anything else (denied, no result)
    // is not a successful dispatch.
    if matches!(decision, GateDecision::Allow) && result.is_some() {
        crate::pipeline::notify_owner_best_effort(
            state,
            state.owner_user_id,
            &format!(
                "Standing rule {} dark-window fired: owner silence = pre-agreed consent; action dispatched.",
                pending.rule_id,
            ),
        )
        .await;
    }
    Ok(())
}

/// Re-drive any fired default whose one-use token was never consumed (a crash
/// between claim and dispatch), and surface any claimed-but-unattempted effect
/// for owner attention. Rows whose token was already consumed but the effect
/// was never durably attempted (`claimed`) are NOT re-dispatched — recovery
/// records them for owner investigation (fail closed, no blind retry of a
/// possibly-completed effect, P1-10/P1-11/D-073). A corrupt/missing payload
/// artifact on a re-drivable (`none`) row propagates as an error rather than
/// being silently swallowed.
async fn recover_unredriven_pending(state: &AppState) -> Result<(), StoreError> {
    // 1. Surface claimed-but-unattempted effects for owner attention.
    let claimed = state.store.pending_dark_window_claimed_unredriven()?;
    for pending in claimed {
        if let Err(err) = state
            .store
            .surface_dark_window_claimed_for_owner(&pending.pending_id, Timestamp::now())
        {
            tracing::error!(
                error = %err,
                pending_id = %pending.pending_id,
                "standing-rule dark-window claimed-effect surface failed"
            );
            continue;
        }
        crate::pipeline::notify_owner_best_effort(
            state,
            state.owner_user_id,
            &format!(
                "Standing rule {} dark-window fired: owner silence = pre-agreed consent, but the \
                 action was not durably confirmed. Investigate pending {} (never auto-rerun).",
                pending.rule_id, pending.pending_id
            ),
        )
        .await;
    }
    // 2. Re-drive token-never-consumed (`none`) rows exactly once.
    let pendings = state.store.pending_dark_window_recoverable()?;
    for pending in pendings {
        if pending.resolution.as_deref() != Some("allowed") {
            continue;
        }
        // Decode the payload BEFORE the grant lookup so a corrupt/missing
        // artifact surfaces as an error (fail closed) regardless of whether the
        // grant still exists — never silently skipped.
        let payload_json = decode_pending_payload(state, &pending.payload_ref)?;
        let Some(grant) = state.store.find_task_grant_by_id(pending.task_grant_id)? else {
            continue;
        };
        let _ = mediate_and_dispatch_action(
            state,
            &grant.0,
            pending.action_id.clone(),
            pending.bound_chat_id,
            payload_json.as_ref(),
            crate::api::actions::FailureSurface::Detached,
            Some(&pending.pending_id),
        )
        .await
        .map_err(|e| {
            StoreError::FailureRouting(format!("dark-window recovery redispatch failed: {e:?}"))
        });
    }
    Ok(())
}
/// Shared by the consumer and the replay test so the parser is exercised
/// exactly once. Returns `None` for events that are not standing-rule timers
/// (e.g. task deadlines), so the consumer can skip them without disturbing
/// its own checkpoint.
pub fn standing_rule_timer_id_from_event(
    event: &openspine_schemas::audit::AuditEvent,
) -> Option<String> {
    event
        .payload_json
        .as_deref()
        .and_then(|payload| serde_json::from_str::<serde_json::Value>(payload).ok())
        .and_then(|payload| {
            payload
                .get("timer_id")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        })
}

// `ArtifactRef`/`ActionId`/`Ulid` are used as types in `decode_pending_payload`
// and `recover_unredriven_pending` above, so their imports stay live.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::dispatch_tests::{mint_grant_with_selection_token, OWNER_CHAT_ID};
    use crate::pipeline::handle_owner_update;
    use crate::store::standing_rules::standing_rule_fingerprint;
    use crate::store::standing_rules_tests::manifest;
    use crate::telegram::TelegramConnector;
    use crate::test_support::fixtures::{test_state, test_state_with_telegram};
    use jiff::Timestamp;
    use openspine_schemas::action::ActionId;
    use openspine_schemas::artifact::ArtifactRef;
    use openspine_schemas::digest::{canonical_json, digest_of_bytes};
    use openspine_schemas::standing_rule::{BudgetWindow, DarkWindowConfig, DarkWindowDefault};
    use rusqlite::params;
    use serde_json::json;
    use ulid::Ulid;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn decode_missing_payload_ref_propagates_error() {
        // P2: a missing/corrupt encrypted payload artifact must surface as an
        // error, never decode to `None` and silently drop the pending row.
        let state = test_state();
        let missing = ArtifactRef {
            digest: digest_of_bytes(b"does-not-exist-blob"),
            schema_version: 1,
        };
        let err = decode_pending_payload(&state, &Some(missing));
        assert!(
            err.is_err(),
            "missing artifact ref must surface as error, not None"
        );
    }

    #[tokio::test]
    async fn recovery_surfaces_claimed_and_propagates_missing_none_payload() {
        // P1/D-073: a `claimed` (receiptless) fired default is surfaced for
        // owner attention exactly once and never re-dispatched. P2: a `none`
        // pending whose payload artifact is missing makes recovery return
        // `Err` (fail closed) instead of silently skipping it.
        let state = test_state();
        let manifest = manifest(
            "rule-timer-recovery",
            "timer.recovery.action",
            7 * 24 * 3600,
            BudgetWindow {
                max: 5,
                window_secs: 7 * 24 * 3600,
            },
            BudgetWindow {
                max: 5,
                window_secs: 3600,
            },
            Some(DarkWindowConfig {
                timeout_secs: 60,
                default: DarkWindowDefault::Allow,
            }),
        );
        let now = Timestamp::from_second(3_000_000).unwrap();
        state
            .store
            .activate_standing_rule(&manifest, None, now)
            .unwrap();
        let rule = state
            .store
            .active_standing_rule_for_action(&ActionId::new("timer.recovery.action"), now)
            .unwrap()
            .unwrap();
        let grant_id = Ulid::new();
        let chat = 77;
        let payload_ref = Some(ArtifactRef {
            digest: digest_of_bytes(b"encrypted action payload"),
            schema_version: 1,
        });
        let fingerprint = standing_rule_fingerprint(&rule.action_id, grant_id, chat, &payload_ref);
        let timer_id = state
            .store
            .schedule_standing_rule_dark_window(
                &rule,
                grant_id,
                chat,
                payload_ref.clone(),
                &fingerprint,
                now + std::time::Duration::from_secs(60),
                now,
            )
            .unwrap()
            .unwrap();
        let pending = state
            .store
            .claim_standing_rule_dark_window(&timer_id, now + std::time::Duration::from_secs(60))
            .unwrap()
            .unwrap();
        // Token consumed -> `claimed` (receiptless). Recovery must surface it.
        let consumed = state
            .store
            .consume_standing_rule_fired_pending(
                &pending.pending_id,
                &rule.action_id,
                grant_id,
                chat,
                &payload_ref,
                now + std::time::Duration::from_secs(61),
            )
            .unwrap();
        assert!(consumed.is_some());
        recover_unredriven_pending(&state).await.unwrap();
        // Surfaced exactly once.
        assert_eq!(
            state
                .store
                .count_audit_events_of_kind("standing_rule.dark_window_effect_unconfirmed")
                .unwrap(),
            1
        );
        // Never auto-dispatched: still `claimed`.
        let ds: String = state
            .store
            .conn
            .lock()
            .query_row(
                "SELECT dispatch_state FROM standing_rule_pending_actions WHERE pending_id = ?1",
                params![pending.pending_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ds, "claimed");

        // A `none` pending with a missing payload ref must make recovery return
        // `Err` (fail closed), not silently skip it.
        let grant2 = Ulid::new();
        let chat2 = 78;
        let fp2 = standing_rule_fingerprint(&rule.action_id, grant2, chat2, &payload_ref);
        let timer2 = state
            .store
            .schedule_standing_rule_dark_window(
                &rule,
                grant2,
                chat2,
                payload_ref.clone(),
                &fp2,
                now + std::time::Duration::from_secs(60),
                now,
            )
            .unwrap()
            .unwrap();
        let pending2 = state
            .store
            .claim_standing_rule_dark_window(&timer2, now + std::time::Duration::from_secs(60))
            .unwrap()
            .unwrap();
        let missing = ArtifactRef {
            digest: digest_of_bytes(b"missing-blob-timer"),
            schema_version: 1,
        };
        let missing_json = serde_json::to_string(&missing).unwrap();
        state
            .store
            .conn
            .lock()
            .execute(
                "UPDATE standing_rule_pending_actions SET payload_ref_json = ?2 WHERE pending_id = ?1",
                params![pending2.pending_id, missing_json],
            )
            .unwrap();
        let result = recover_unredriven_pending(&state).await;
        assert!(
            result.is_err(),
            "missing payload on a none pending must propagate as error"
        );
    }

    #[tokio::test]
    async fn standing_rule_timer_redelivery_dispatches_once() {
        // D-082: a replayed `workflow.timer_fired` event (the same timer
        // redelivered to the consumer) must apply the fired default exactly
        // once — the second claim finds the timer already applied and is a
        // no-op, so the effect is never double-dispatched.
        let state = test_state();
        let now = Timestamp::now();
        // The pending payload blob must exist in the artifact store so the
        // consumer can decode it for re-dispatch.
        let _ = state.artifacts.put(br#"{}"#.as_slice()).unwrap();
        let manifest = manifest(
            "rule-timer-redelivery",
            "connector.enable",
            7 * 24 * 3600,
            BudgetWindow {
                max: 5,
                window_secs: 7 * 24 * 3600,
            },
            BudgetWindow {
                max: 5,
                window_secs: 3600,
            },
            Some(DarkWindowConfig {
                timeout_secs: 60,
                default: DarkWindowDefault::Allow,
            }),
        );
        state
            .store
            .activate_standing_rule(&manifest, None, now)
            .unwrap();
        let rule = state
            .store
            .active_standing_rule_for_action(&ActionId::new("connector.enable"), now)
            .unwrap()
            .unwrap();
        let grant = handle_owner_update(
            &state,
            &crate::test_support::fixtures::owner_update("enable something"),
        )
        .await
        .unwrap()
        .expect("owner update must compose a grant");
        let grant_id = grant.id;
        let chat = 555;
        let payload_ref = Some(ArtifactRef {
            digest: digest_of_bytes(br#"{}"#),
            schema_version: 1,
        });
        let fingerprint = standing_rule_fingerprint(&rule.action_id, grant_id, chat, &payload_ref);
        let timer_id = state
            .store
            .schedule_standing_rule_dark_window(
                &rule,
                grant_id,
                chat,
                payload_ref.clone(),
                &fingerprint,
                now + std::time::Duration::from_secs(60),
                now,
            )
            .unwrap()
            .unwrap();
        // First delivery: claim + redispatch consumes the one-use token and
        // dispatches the effect.
        claim_and_redispatch(&state, &timer_id, now + std::time::Duration::from_secs(61))
            .await
            .unwrap();
        assert_eq!(
            state
                .store
                .count_audit_events_of_kind("standing_rule.dark_window_admitted")
                .unwrap(),
            1,
            "the fired default was admitted exactly once"
        );
        // Second delivery of the SAME timer event: the timer is already
        // applied, so the claim is a no-op and no second effect runs.
        claim_and_redispatch(&state, &timer_id, now + std::time::Duration::from_secs(62))
            .await
            .unwrap();
        assert_eq!(
            state
                .store
                .count_audit_events_of_kind("standing_rule.dark_window_admitted")
                .unwrap(),
            1,
            "a replayed timer event must not double-dispatch the effect"
        );
        // Exactly one budget unit was consumed, never two.
        let committed: i64 = state
            .store
            .conn
            .lock()
            .query_row(
                "SELECT COUNT(DISTINCT reservation_id) FROM standing_rule_usage WHERE rule_id = ?1 AND status = 'committed'",
                params![rule.rule_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(committed, 1, "the token was consumed exactly once");
    }
    #[tokio::test]
    async fn fired_connector_pre_effect_failure_rearms_then_retries_once() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/bottest-token/SendMessage"))
            .respond_with(ResponseTemplate::new(500).set_body_json(json!({
                "ok": false,
                "description": "synthetic connector failure"
            })))
            .with_priority(1)
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/bottest-token/SendMessage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok": true,
                "result": {
                    "message_id": 1,
                    "date": 0,
                    "chat": {"id": OWNER_CHAT_ID, "type": "private"},
                    "text": "retry fired default"
                }
            })))
            .with_priority(2)
            .mount(&server)
            .await;
        let state = test_state_with_telegram(TelegramConnector::with_api_url(
            "test-token".to_string(),
            server.uri().parse().unwrap(),
        ));
        let now = Timestamp::now();
        let manifest = manifest(
            "rule-fired-connector-retry",
            "telegram.reply:owner_channel",
            3600,
            BudgetWindow {
                max: 5,
                window_secs: 3600,
            },
            BudgetWindow {
                max: 5,
                window_secs: 3600,
            },
            Some(DarkWindowConfig {
                timeout_secs: 60,
                default: DarkWindowDefault::Allow,
            }),
        );
        state
            .store
            .activate_standing_rule(&manifest, None, now)
            .unwrap();
        let rule = state
            .store
            .active_standing_rule_for_action(&manifest.action_id, now)
            .unwrap()
            .unwrap();
        let mut grant = mint_grant_with_selection_token(
            &state,
            &["telegram.reply:owner_channel"],
            now + std::time::Duration::from_secs(120),
        )
        .0;
        grant.approval_required_actions = vec![ActionId::new("telegram.reply:owner_channel")];
        grant.seal_root(b"openspine-test-grant-hmac-key-v1");
        let mut stored = grant.clone();
        stored.task_token.clear();
        state
            .store
            .conn
            .lock()
            .execute(
                "UPDATE task_grants SET grant_json = ?2 WHERE id = ?1",
                params![
                    grant.id.to_string(),
                    serde_json::to_string(&stored).unwrap()
                ],
            )
            .unwrap();
        let payload = json!({"text": "retry fired default"});
        let payload_ref = Some(
            state
                .artifacts
                .put(canonical_json(&payload).as_bytes())
                .unwrap(),
        );
        let fingerprint =
            standing_rule_fingerprint(&rule.action_id, grant.id, OWNER_CHAT_ID, &payload_ref);
        let timer_id = state
            .store
            .schedule_standing_rule_dark_window(
                &rule,
                grant.id,
                OWNER_CHAT_ID,
                payload_ref,
                &fingerprint,
                now + std::time::Duration::from_secs(60),
                now,
            )
            .unwrap()
            .unwrap();

        let first =
            claim_and_redispatch(&state, &timer_id, now + std::time::Duration::from_secs(61)).await;
        assert!(
            first.is_err(),
            "the first connector failure must surface for timer retry"
        );
        let fired_gated_after_failure = state
            .store
            .all_audit_event_jsons()
            .unwrap()
            .into_iter()
            .filter_map(|event| serde_json::from_str::<serde_json::Value>(&event).ok())
            .filter(|event| {
                event["kind"] == "action.gated"
                    && event["reason"]
                        .as_str()
                        .is_some_and(|reason| reason.contains("fired dark-window default admitted"))
            })
            .count();
        assert_eq!(
            state
                .store
                .standing_rule_remaining(&manifest.id, now)
                .unwrap(),
            (5, 5),
            "pre-effect connector failure refunds the reserved unit"
        );
        recover_unredriven_pending(&state).await.unwrap();
        assert!(
            state
                .store
                .count_audit_events_of_kind("standing_rule.dark_window_admitted")
                .unwrap()
                >= 2,
            "rearmed fired token must be eligible on recovery redelivery"
        );
        let fired_gated_after_retry = state
            .store
            .all_audit_event_jsons()
            .unwrap()
            .into_iter()
            .filter_map(|event| serde_json::from_str::<serde_json::Value>(&event).ok())
            .filter(|event| {
                event["kind"] == "action.gated"
                    && event["reason"]
                        .as_str()
                        .is_some_and(|reason| reason.contains("fired dark-window default admitted"))
            })
            .count();
        assert_eq!(
            fired_gated_after_retry,
            fired_gated_after_failure + 1,
            "rearmed successful firing appends exactly one new effective-Allow action.gated audit"
        );
        assert_eq!(
            state
                .store
                .count_audit_events_of_kind("standing_rule.dark_window_admitted")
                .unwrap(),
            2,
            "the default is admitted once per actual dispatch attempt"
        );
        assert_eq!(
            state
                .store
                .standing_rule_remaining(&manifest.id, now)
                .unwrap(),
            (4, 4),
            "only the successful retry consumes budget"
        );
    }
}
