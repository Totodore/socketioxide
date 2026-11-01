//! The websocket inbound limits must follow [`EngineIoConfig::max_payload`] unless they are
//! explicitly configured. Otherwise a websocket client can bypass the limit applied to polling.

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
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        Message,
        protocol::frame::{
            Frame,
            coding::{Data, OpCode},
        },
    },
};

mod fixture;
use fixture::{StreamImpl, create_ws_connection};

const TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug)]
struct MyHandler {
    msg_tx: mpsc::UnboundedSender<usize>,
    disconnect_tx: mpsc::UnboundedSender<DisconnectReason>,
}

impl EngineIoHandler for MyHandler {
    type Data = ();

    fn on_connect(self: Arc<Self>, _socket: Arc<Socket<()>>) {}
    fn on_disconnect(&self, _socket: Arc<Socket<()>>, reason: DisconnectReason) {
        self.disconnect_tx.send(reason).unwrap();
    }
    fn on_message(self: &Arc<Self>, msg: Str, _socket: Arc<Socket<()>>) {
        self.msg_tx.send(msg.len()).unwrap();
    }
    fn on_binary(self: &Arc<Self>, data: Bytes, _socket: Arc<Socket<()>>) {
        self.msg_tx.send(data.len()).unwrap();
    }
}

struct Server {
    msg_rx: mpsc::UnboundedReceiver<usize>,
    disconnect_rx: mpsc::UnboundedReceiver<DisconnectReason>,
    conn: WebSocketStream<StreamImpl>,
}

async fn create_server(mut config: EngineIoConfig) -> Server {
    // We don't want any heartbeat to interfere with the tests.
    config.ping_interval = Duration::from_secs(60);
    config.ping_timeout = Duration::from_secs(60);

    let (msg_tx, msg_rx) = mpsc::unbounded_channel();
    let (disconnect_tx, disconnect_rx) = mpsc::unbounded_channel();
    let handler = MyHandler {
        msg_tx,
        disconnect_tx,
    };
    let mut svc = EngineIoService::with_config(Arc::new(handler), config);
    let conn = create_ws_connection(&mut svc).await;
    Server {
        msg_rx,
        disconnect_rx,
        conn,
    }
}

/// A text message of exactly `len` bytes on the wire, packet type included.
fn message(len: usize) -> Message {
    Message::text(format!("4{}", "a".repeat(len - 1)))
}

impl Server {
    /// Assert that the message reached the handler and that the connection is still alive.
    async fn assert_received(&mut self, len: usize) {
        let received = tokio::time::timeout(TIMEOUT, self.msg_rx.recv())
            .await
            .expect("the message should have reached the handler")
            .unwrap();
        assert_eq!(received, len - 1);
        assert!(self.disconnect_rx.try_recv().is_err());
    }

    /// Assert that the session was closed without the handler seeing any message.
    async fn assert_rejected(&mut self) {
        tokio::time::timeout(TIMEOUT, self.disconnect_rx.recv())
            .await
            .expect("the session should have been closed")
            .unwrap();
        assert!(self.msg_rx.try_recv().is_err());
    }
}

#[tokio::test]
async fn default_limit_is_max_payload() {
    let config = EngineIoConfig::default();
    assert_eq!(config.max_payload, 100_000);

    let mut server = create_server(config.clone()).await;
    server.conn.send(message(100_000)).await.unwrap();
    server.assert_received(100_000).await;

    let mut server = create_server(config).await;
    server.conn.send(message(100_001)).await.unwrap();
    server.assert_rejected().await;
}

#[tokio::test]
async fn builder_limit_follows_max_payload() {
    let config = EngineIoConfig::builder().max_payload(1_000).build();
    let mut server = create_server(config.clone()).await;
    server.conn.send(message(1_000)).await.unwrap();
    server.assert_received(1_000).await;

    let mut server = create_server(config).await;
    server.conn.send(message(1_001)).await.unwrap();
    server.assert_rejected().await;
}

#[tokio::test]
async fn binary_message_over_limit_is_rejected() {
    let mut server = create_server(EngineIoConfig::default()).await;
    let data = Bytes::from(vec![0u8; 100_001]);
    server.conn.send(Message::binary(data)).await.unwrap();
    server.assert_rejected().await;
}

/// Each frame is within the frame limit, but the reassembled message is not.
#[tokio::test]
async fn fragmented_message_over_limit_is_rejected() {
    let mut server = create_server(EngineIoConfig::default()).await;
    let chunk = "a".repeat(60_000);

    let first = Frame::message(
        format!("4{chunk}").into_bytes(),
        OpCode::Data(Data::Text),
        false,
    );
    let last = Frame::message(chunk.into_bytes(), OpCode::Data(Data::Continue), true);
    server.conn.feed(Message::Frame(first)).await.unwrap();
    server.conn.send(Message::Frame(last)).await.unwrap();
    server.assert_rejected().await;
}

#[tokio::test]
async fn explicit_ws_limit_overrides_max_payload() {
    // A config with a websocket transport more permissive than the polling one
    // is still possible, and the order of the builder calls must not matter.
    let config = EngineIoConfig::builder()
        .ws_max_message_size(200_000)
        .max_payload(1_000)
        .build();
    let mut server = create_server(config).await;
    server.conn.send(message(150_000)).await.unwrap();
    server.assert_received(150_000).await;
}
