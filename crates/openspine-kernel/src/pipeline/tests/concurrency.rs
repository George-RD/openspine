use crate::pipeline::handle_owner_update;
use crate::telegram::{CallbackQueryUpdate, TelegramConnector, TelegramUpdate};
use crate::test_support::fixtures::test_state;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

struct FirstCallbackGate {
    first: AtomicBool,
    first_seen: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    second_seen: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    release: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}

impl Respond for FirstCallbackGate {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        if self.first.swap(false, Ordering::SeqCst) {
            self.first_seen
                .lock()
                .expect("first callback signal lock")
                .take()
                .expect("first callback signal")
                .send(())
                .expect("first callback receiver");
            self.release
                .lock()
                .expect("callback gate lock")
                .take()
                .expect("callback release receiver")
                .recv()
                .expect("callback release signal");
        } else {
            self.second_seen
                .lock()
                .expect("second callback signal lock")
                .take()
                .expect("second callback signal")
                .send(())
                .expect("second callback receiver");
        }
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "result": true
        }))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_owner_callbacks_are_serialized_by_handler() {
    let server = MockServer::start().await;
    let (first_seen_tx, first_seen_rx) = std::sync::mpsc::channel();
    let (second_seen_tx, second_seen_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    Mock::given(method("POST"))
        .and(path("/bottest-token/AnswerCallbackQuery"))
        .respond_with(FirstCallbackGate {
            first: AtomicBool::new(true),
            first_seen: Mutex::new(Some(first_seen_tx)),
            second_seen: Mutex::new(Some(second_seen_tx)),
            release: Mutex::new(Some(release_rx)),
        })
        .mount(&server)
        .await;

    let mut state = test_state();
    state.connectors = crate::connectors::ConnectorRegistry::new(
        TelegramConnector::with_api_url("test-token".to_string(), server.uri().parse().unwrap()),
        None,
    )
    .expect("built-in egress ratings are conflict-free");
    let state = Arc::new(state);

    let callback = |id: &str| TelegramUpdate {
        update_id: id.parse().unwrap(),
        chat_id: 555,
        is_private_chat: true,
        sender_user_id: Some(42),
        callback_query: Some(CallbackQueryUpdate {
            id: id.to_string(),
            data: Some("unrecognized_callback".to_string()),
        }),
        ..Default::default()
    };
    let first_update = callback("1");
    let second_update = callback("2");

    let first_state = Arc::clone(&state);
    let first = tokio::spawn(async move { handle_owner_update(&first_state, &first_update).await });
    tokio::time::timeout(Duration::from_secs(1), async {
        tokio::task::spawn_blocking(move || first_seen_rx.recv().expect("first callback"))
            .await
            .expect("first callback signal task");
    })
    .await
    .expect("first callback must reach Telegram before the second starts");

    let second_state = Arc::clone(&state);
    let second =
        tokio::spawn(async move { handle_owner_update(&second_state, &second_update).await });
    assert!(
        tokio::time::timeout(
            Duration::from_millis(100),
            tokio::task::spawn_blocking(move || second_seen_rx.recv())
        )
        .await
        .is_err(),
        "second callback reached Telegram while the first callback was gated"
    );

    release_tx.send(()).unwrap();
    first.await.unwrap().unwrap();
    second.await.unwrap().unwrap();
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
    assert_eq!(
        state
            .store
            .count_audit_events_of_kind("telegram.callback_unrecognized")
            .unwrap(),
        2
    );
}
