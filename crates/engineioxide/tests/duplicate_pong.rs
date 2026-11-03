//! The heartbeat channel holds a single pong. A second one received before the heartbeat job
//! consumes the first one must not be reported as a heartbeat timeout: the client is alive.

use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use engineioxide::{
    Str,
    config::EngineIoConfig,
    handler::EngineIoHandler,
    service::EngineIoService,
    socket::{DisconnectReason, Socket},
};
use futures_util::SinkExt;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

mod fixture;
use fixture::{create_polling_connection, create_ws_connection, send_req};

/// The `ok` response of a polling POST request, without the first byte removed by `send_req`.
const OK_RESPONSE: &str = "k";

#[derive(Debug)]
struct MyHandler {
    disconnect_tx: mpsc::UnboundedSender<DisconnectReason>,
}

impl EngineIoHandler for MyHandler {
    type Data = ();
    fn on_connect(self: Arc<Self>, _: Arc<Socket<()>>) {}
    fn on_disconnect(&self, _: Arc<Socket<()>>, reason: DisconnectReason) {
        self.disconnect_tx.send(reason).unwrap();
    }
    fn on_message(self: &Arc<Self>, _: Str, _: Arc<Socket<()>>) {}
    fn on_binary(self: &Arc<Self>, _: Bytes, _: Arc<Socket<()>>) {}
}

/// The heartbeat job is parked for the whole test, and never consumes the pongs.
fn create_server() -> (
    EngineIoService<MyHandler>,
    mpsc::UnboundedReceiver<DisconnectReason>,
) {
    let (disconnect_tx, disconnect_rx) = mpsc::unbounded_channel();
    let config = EngineIoConfig::builder()
        .ping_interval(Duration::from_secs(60))
        .ping_timeout(Duration::from_secs(60))
        .build();
    let svc = EngineIoService::with_config(Arc::new(MyHandler { disconnect_tx }), config);
    (svc, disconnect_rx)
}

async fn assert_not_closed(disconnect_rx: &mut mpsc::UnboundedReceiver<DisconnectReason>) {
    let res = tokio::time::timeout(Duration::from_millis(100), disconnect_rx.recv()).await;
    assert!(res.is_err(), "the session should not be closed: {res:?}");
}

#[tokio::test]
async fn polling_pongs_in_a_single_request() {
    let (mut svc, mut disconnect_rx) = create_server();
    let sid = create_polling_connection(&mut svc).await;

    let params = format!("transport=polling&sid={sid}");
    let body = Some("3\x1e3\x1e3".to_string());
    let res = send_req(&mut svc, params, http::Method::POST, body).await;

    assert_eq!(res, OK_RESPONSE);
    assert_not_closed(&mut disconnect_rx).await;
}

#[tokio::test]
async fn polling_pongs_in_several_requests() {
    let (mut svc, mut disconnect_rx) = create_server();
    let sid = create_polling_connection(&mut svc).await;

    for _ in 0..3 {
        let params = format!("transport=polling&sid={sid}");
        let res = send_req(&mut svc, params, http::Method::POST, Some("3".into())).await;
        assert_eq!(res, OK_RESPONSE);
    }

    assert_not_closed(&mut disconnect_rx).await;
}

#[tokio::test]
async fn websocket_pongs() {
    let (mut svc, mut disconnect_rx) = create_server();
    let mut ws = create_ws_connection(&mut svc).await;

    for _ in 0..3 {
        ws.send(Message::text("3")).await.unwrap();
    }

    assert_not_closed(&mut disconnect_rx).await;
}

/// Extra pongs must not prevent the detection of a client that stopped answering.
#[tokio::test]
async fn heartbeat_still_times_out_without_a_new_pong() {
    let (disconnect_tx, mut disconnect_rx) = mpsc::unbounded_channel();
    let config = EngineIoConfig::builder()
        .ping_interval(Duration::from_millis(20))
        .ping_timeout(Duration::from_millis(20))
        .build();
    let mut svc = EngineIoService::with_config(Arc::new(MyHandler { disconnect_tx }), config);
    let mut ws = create_ws_connection(&mut svc).await;
    ws.send(Message::text("3")).await.unwrap();
    ws.send(Message::text("3")).await.unwrap();

    let reason = tokio::time::timeout(Duration::from_secs(1), disconnect_rx.recv())
        .await
        .expect("the session should be closed")
        .unwrap();
    assert_eq!(reason, DisconnectReason::HeartbeatTimeout);
}
