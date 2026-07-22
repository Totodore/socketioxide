//! Tests ensuring the configured `max_payload` is enforced on the websocket
//! transport: an inbound message larger than the limit must close the
//! connection instead of riding tungstenite's ~64 MiB defaults, while
//! messages within the limit keep flowing.

use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use engineioxide::{
    Str,
    config::EngineIoConfig,
    handler::EngineIoHandler,
    service::EngineIoService,
    socket::{DisconnectReason, Socket},
};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

mod fixture;

use fixture::create_ws_connection;

#[derive(Debug, Clone)]
struct MyHandler {
    message_tx: mpsc::UnboundedSender<Str>,
    disconnect_tx: mpsc::UnboundedSender<DisconnectReason>,
}

impl EngineIoHandler for MyHandler {
    type Data = ();

    fn on_connect(self: Arc<Self>, _socket: Arc<Socket<()>>) {}
    fn on_disconnect(&self, _socket: Arc<Socket<()>>, reason: DisconnectReason) {
        self.disconnect_tx.send(reason).unwrap();
    }
    fn on_message(self: &Arc<Self>, msg: Str, _socket: Arc<Socket<()>>) {
        self.message_tx.send(msg).unwrap();
    }
    fn on_binary(self: &Arc<Self>, _data: Bytes, _socket: Arc<Socket<()>>) {}
}

/// Build a service with a small `max_payload` and heartbeat timings large
/// enough not to interfere.
fn create_max_payload_server(
    max_payload: u64,
) -> (
    EngineIoService<MyHandler>,
    mpsc::UnboundedReceiver<Str>,
    mpsc::UnboundedReceiver<DisconnectReason>,
) {
    let (message_tx, message_rx) = mpsc::unbounded_channel();
    let (disconnect_tx, disconnect_rx) = mpsc::unbounded_channel();
    let config = EngineIoConfig::builder()
        .ping_interval(Duration::from_secs(60))
        .ping_timeout(Duration::from_secs(60))
        .max_payload(max_payload)
        .build();
    let svc = EngineIoService::with_config(
        Arc::new(MyHandler {
            message_tx,
            disconnect_tx,
        }),
        config,
    );
    (svc, message_rx, disconnect_rx)
}

#[tokio::test]
async fn ws_message_within_max_payload_is_delivered() {
    let (mut svc, mut messages, _disconnects) = create_max_payload_server(1024);
    let mut ws = create_ws_connection(&mut svc).await;
    let open = ws.next().await.unwrap().unwrap();
    assert!(open.into_text().unwrap().starts_with('0'));

    ws.send(Message::Text("4hello".into())).await.unwrap();

    let msg = tokio::time::timeout(Duration::from_secs(1), messages.recv())
        .await
        .expect("message within max_payload must be delivered")
        .unwrap();
    assert_eq!(&*msg, "hello");
}

#[tokio::test]
async fn ws_message_exceeding_max_payload_closes_the_connection() {
    let (mut svc, mut messages, mut disconnects) = create_max_payload_server(1024);
    let mut ws = create_ws_connection(&mut svc).await;
    let open = ws.next().await.unwrap().unwrap();
    assert!(open.into_text().unwrap().starts_with('0'));

    let oversized = format!("4{}", "x".repeat(2048));
    ws.send(Message::Text(oversized.into())).await.unwrap();

    let reason = tokio::time::timeout(Duration::from_secs(1), disconnects.recv())
        .await
        .expect("an oversized message must disconnect the socket")
        .unwrap();
    assert!(
        matches!(reason, DisconnectReason::TransportError),
        "expected a transport error, got: {reason:?}"
    );
    assert!(
        messages.try_recv().is_err(),
        "the oversized message must not reach the handler"
    );
}
