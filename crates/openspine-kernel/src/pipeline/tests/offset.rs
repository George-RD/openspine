use crate::test_support::fixtures::*;

#[tokio::test]
async fn telegram_offset_reads_namespaced_consumed_offset_for_same_bot() {
    let state = test_state();
    state
        .store
        .set_kv("last_telegram_update_id.777", "100")
        .unwrap();
    state.store.set_kv("telegram.bot_id", "777").unwrap();
    let (key, id) = crate::pipeline::resolve_telegram_offset(&state).unwrap();
    assert_eq!(key, "last_telegram_update_id.777");
    assert_eq!(id, Some(100));
    let (key2, id2) = crate::pipeline::resolve_telegram_offset(&state).unwrap();
    assert_eq!(key2, key);
    assert_eq!(id2, Some(100));
}

#[tokio::test]
async fn consumed_update_is_dropped_before_pipeline_dispatch() {
    let state = test_state();
    state
        .store
        .set_kv("last_telegram_update_id.777", "100")
        .unwrap();
    state.store.set_kv("telegram.bot_id", "777").unwrap();
    let (offset_key, last) = crate::pipeline::resolve_telegram_offset(&state).unwrap();
    let mut consumed = owner_update("/secret rotate telegram.bot_token");
    consumed.update_id = 100;
    let dispatched = crate::pipeline::polling::dispatch_polled_updates(
        &state,
        vec![consumed],
        offset_key.clone(),
        last,
    )
    .await
    .unwrap();
    assert_eq!(dispatched, 0);
    assert_eq!(state.store.count_task_grants().unwrap(), 0);
    let mut fresh = owner_update("hello lyra");
    fresh.update_id = 101;
    let dispatched2 =
        crate::pipeline::polling::dispatch_polled_updates(&state, vec![fresh], offset_key, last)
            .await
            .unwrap();
    assert_eq!(dispatched2, 1);
}

#[tokio::test]
async fn different_bot_rotation_starts_fresh_namespace() {
    let state = test_state();
    state
        .store
        .set_kv("last_telegram_update_id", "100")
        .unwrap();
    state.store.set_kv("telegram.bot_id", "888").unwrap();
    let (key, id) = crate::pipeline::resolve_telegram_offset(&state).unwrap();
    assert_eq!(key, "last_telegram_update_id.888");
    assert_eq!(id, None);
}

#[test]
fn replay_guard_filters_consumed_update_ids() {
    assert!(crate::pipeline::is_already_processed(100, Some(100)));
    assert!(crate::pipeline::is_already_processed(50, Some(100)));
    assert!(!crate::pipeline::is_already_processed(101, Some(100)));
    assert!(!crate::pipeline::is_already_processed(1, None));
}
