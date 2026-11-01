//! A websocket client that stops reading must not prevent its session from being closed.
//!
//! The socket writer is parked as soon as the transport can't accept more data. It holds the
//! `internal_rx` lock, on which the end of the session depends: nothing else
//! would release it, and the disconnect handler would never be called.

use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use engineioxide::{
    ProtocolVersion, Str,
    config::EngineIoConfig,
    handler::EngineIoHandler,
    service::EngineIoService,
    socket::{DisconnectReason, Socket},
};
use futures_util::StreamExt;
use http::Request;
use tokio::{io::DuplexStream, sync::mpsc};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, handshake::client::generate_key, protocol::Role},
};

const TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug)]
struct MyHandler {
    socket_tx: mpsc::UnboundedSender<Arc<Socket<()>>>,
    disconnect_tx: mpsc::UnboundedSender<DisconnectReason>,
}

impl EngineIoHandler for MyHandler {
    type Data = ();

    fn on_connect(self: Arc<Self>, socket: Arc<Socket<()>>) {
        self.socket_tx.send(socket).unwrap();
    }
    fn on_disconnect(&self, _socket: Arc<Socket<()>>, reason: DisconnectReason) {
        self.disconnect_tx.send(reason).unwrap();
    }
    fn on_message(self: &Arc<Self>, _msg: Str, _socket: Arc<Socket<()>>) {}
    fn on_binary(self: &Arc<Self>, _data: Bytes, _socket: Arc<Socket<()>>) {}
}

struct Server {
    socket: Arc<Socket<()>>,
    disconnect_rx: mpsc::UnboundedReceiver<DisconnectReason>,
    /// The client side of the connection. It is not read until a test decides so.
    client: WebSocketStream<DuplexStream>,
}

/// Connect a client through a small pipe, which blocks the server writes when it is not read.
async fn connect(config: EngineIoConfig) -> Server {
    let (socket_tx, mut socket_rx) = mpsc::unbounded_channel();
    let (disconnect_tx, disconnect_rx) = mpsc::unbounded_channel();
    let handler = MyHandler {
        socket_tx,
        disconnect_tx,
    };
    let svc = EngineIoService::with_config(Arc::new(handler), config);

    let (client_io, server_io) = tokio::io::duplex(1024);
    let parts = Request::builder()
        .method("GET")
        .header("Host", "127.0.0.1")
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", generate_key())
        .uri("ws://127.0.0.1/engine.io/?EIO=4&transport=websocket")
        .body(())
        .unwrap()
        .into_parts()
        .0;
    tokio::spawn(svc.ws_init(server_io, ProtocolVersion::V4, None, parts));

    let client = WebSocketStream::from_raw_socket(client_io, Role::Client, None).await;
    let socket = tokio::time::timeout(TIMEOUT, socket_rx.recv())
        .await
        .unwrap()
        .unwrap();
    Server {
        socket,
        disconnect_rx,
        client,
    }
}

/// Emit more data than the pipe and the write buffer of the websocket can hold.
async fn park_writer(socket: &Socket<()>) {
    for _ in 0..8 {
        socket
            .emit_binary(Bytes::from(vec![0u8; 256 * 1024]))
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
}

fn long_heartbeat() -> EngineIoConfig {
    EngineIoConfig::builder()
        .ping_interval(Duration::from_secs(60))
        .ping_timeout(Duration::from_secs(60))
        .build()
}

#[tokio::test]
async fn closing_a_session_with_a_parked_writer_calls_the_disconnect_handler() {
    let mut server = connect(long_heartbeat()).await;
    park_writer(&server.socket).await;

    server.socket.close(DisconnectReason::TransportClose);

    let reason = tokio::time::timeout(TIMEOUT, server.disconnect_rx.recv())
        .await
        .expect("the disconnect handler should have been called")
        .unwrap();
    assert_eq!(reason, DisconnectReason::TransportClose);
}

#[tokio::test]
async fn heartbeat_timeout_with_a_parked_writer_calls_the_disconnect_handler() {
    let config = EngineIoConfig::builder()
        .ping_interval(Duration::from_millis(20))
        .ping_timeout(Duration::from_millis(20))
        .build();
    let mut server = connect(config).await;
    park_writer(&server.socket).await;

    let reason = tokio::time::timeout(TIMEOUT, server.disconnect_rx.recv())
        .await
        .expect("the disconnect handler should have been called")
        .unwrap();
    assert_eq!(reason, DisconnectReason::HeartbeatTimeout);
}

/// The connection must not stay open forever after the session is closed.
#[tokio::test]
async fn closing_a_session_with_a_parked_writer_closes_the_connection() {
    let mut server = connect(long_heartbeat()).await;
    park_writer(&server.socket).await;
    server.socket.close(DisconnectReason::TransportClose);
    tokio::time::timeout(TIMEOUT, server.disconnect_rx.recv())
        .await
        .expect("the disconnect handler should have been called")
        .unwrap();

    // Let the bounded close attempt fail before reading.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    tokio::time::timeout(TIMEOUT, async {
        while let Some(Ok(_)) = server.client.next().await {}
    })
    .await
    .expect("the connection should have been closed");
}

/// A client that reads its messages must still get everything sent before the end of the session.
#[tokio::test]
async fn closing_a_session_flushes_pending_packets_to_a_healthy_client() {
    let mut server = connect(long_heartbeat()).await;
    server.socket.emit(Str::from("last")).unwrap();
    server.socket.close(DisconnectReason::TransportClose);

    let mut messages = vec![];
    tokio::time::timeout(TIMEOUT, async {
        while let Some(Ok(msg)) = server.client.next().await {
            messages.push(msg);
        }
    })
    .await
    .expect("the connection should have been closed");

    assert!(messages.contains(&Message::text("4last")), "{messages:?}");
    assert!(matches!(messages.last(), Some(Message::Close(_))));
}
