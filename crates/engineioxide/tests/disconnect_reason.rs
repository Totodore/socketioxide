//! Tests for disconnect reasons
//! Test are made on polling and websocket transports:
//! * Heartbeat timeout
//! * Transport close
//! * Multiple http polling
//! * Packet parsing

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

mod fixture;

use fixture::{create_server, create_ws_connection, send_req};
use tokio_tungstenite::tungstenite::Message;

use crate::fixture::{create_polling_connection, create_ws_upgrade_connection};

#[derive(Debug, Clone)]
struct MyHandler {
    disconnect_tx: mpsc::Sender<DisconnectReason>,
}

impl EngineIoHandler for MyHandler {
    type Data = ();

    fn on_connect(self: Arc<Self>, socket: Arc<Socket<()>>) {
        println!("socket connect {}", socket.id);
    }
    fn on_disconnect(&self, socket: Arc<Socket<()>>, reason: DisconnectReason) {
        println!("socket disconnect {}: {:?}", socket.id, reason);
        self.disconnect_tx.try_send(reason).unwrap();
    }

    fn on_message(self: &Arc<Self>, msg: Str, socket: Arc<Socket<()>>) {
        println!("Ping pong message {msg:?}");
        socket.emit(msg).ok();
    }

    fn on_binary(self: &Arc<Self>, data: Bytes, socket: Arc<Socket<()>>) {
        println!("Ping pong binary message {data:?}");
        socket.emit_binary(data).ok();
    }
}

/// A handler that forwards every connected socket to the test so it can drive its lifecycle.
#[derive(Debug, Clone)]
struct CloseHandler {
    socket_tx: mpsc::Sender<Arc<Socket<()>>>,
}

impl EngineIoHandler for CloseHandler {
    type Data = ();

    fn on_connect(self: Arc<Self>, socket: Arc<Socket<()>>) {
        self.socket_tx.try_send(socket).unwrap();
    }
    fn on_disconnect(&self, _socket: Arc<Socket<()>>, _reason: DisconnectReason) {}
    fn on_message(self: &Arc<Self>, _msg: Str, _socket: Arc<Socket<()>>) {}
    fn on_binary(self: &Arc<Self>, _data: Bytes, _socket: Arc<Socket<()>>) {}
}

/// Closing a websocket session should resolve [`Socket::closed`].
#[tokio::test]
pub async fn ws_server_closing() {
    let (socket_tx, mut socket_rx) = mpsc::channel(1);
    let mut svc = create_server(CloseHandler { socket_tx }).await;
    let _stream = create_ws_connection(&mut svc).await;

    let socket = socket_rx.recv().await.unwrap();
    socket.close(DisconnectReason::ClosingServer);

    tokio::time::timeout(Duration::from_millis(100), socket.closed())
        .await
        .expect("timeout: ws session did not close while the forward task held the channel lock");
    assert!(socket.is_closed());
}

/// Same as [`ws_server_closing`] but for polling with a long-poll request in-flight.
#[tokio::test]
pub async fn polling_server_closing_pending_poll() {
    let (socket_tx, mut socket_rx) = mpsc::channel(1);
    let mut svc = create_server(CloseHandler { socket_tx }).await;
    let sid = create_polling_connection(&mut svc).await;
    let socket = socket_rx.recv().await.unwrap();

    // Drain the first buffered ping so the next poll actually blocks.
    send_req(
        &mut svc,
        format!("transport=polling&sid={sid}"),
        http::Method::GET,
        None,
    )
    .await;

    // Start a long-poll: the buffer is empty so it blocks waiting for the next packet while holding
    // the internal channel lock.
    let poll = tokio::spawn(send_req(
        &mut svc,
        format!("transport=polling&sid={sid}"),
        http::Method::GET,
        None,
    ));
    // Give the poll time to acquire the internal channel lock before closing.
    tokio::time::sleep(Duration::from_millis(10)).await;

    socket.close(DisconnectReason::ClosingServer);

    tokio::time::timeout(Duration::from_millis(100), socket.closed())
        .await
        .expect("timeout: polling session did not close while a poll held the channel lock");
    assert!(socket.is_closed());

    // The pending poll should return rather than hang.
    tokio::time::timeout(Duration::from_millis(100), poll)
        .await
        .expect("timeout: the pending poll never returned")
        .unwrap();
}

#[tokio::test]
pub async fn polling_heartbeat_timeout() {
    let (disconnect_tx, mut rx) = mpsc::channel(10);
    let mut svc = create_server(MyHandler { disconnect_tx }).await;
    create_polling_connection(&mut svc).await;

    let data = tokio::time::timeout(Duration::from_millis(500), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::HeartbeatTimeout")
        .unwrap();

    assert_eq!(data, DisconnectReason::HeartbeatTimeout);
}

#[tokio::test]
pub async fn ws_heartbeat_timeout() {
    let (disconnect_tx, mut rx) = mpsc::channel(10);
    let mut svc = create_server(MyHandler { disconnect_tx }).await;
    let _stream = create_ws_connection(&mut svc).await;

    let data = tokio::time::timeout(Duration::from_millis(500), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::HeartbeatTimeout")
        .unwrap();

    assert_eq!(data, DisconnectReason::HeartbeatTimeout);
}

#[tokio::test]
pub async fn ws_upgrade_abandoned_timeout() {
    let (disconnect_tx, mut rx) = mpsc::channel(10);
    // Long heartbeat so it cannot mask the leak during the test window, and a short upgrade
    // timeout so an abandoned upgrade is reclaimed quickly.
    let config = EngineIoConfig::builder()
        .ping_interval(Duration::from_secs(60))
        .ping_timeout(Duration::from_secs(60))
        .upgrade_timeout(Duration::from_millis(200))
        .build();
    let mut svc = EngineIoService::with_config(Arc::new(MyHandler { disconnect_tx }), config);

    // Establish a polling session, then open a websocket upgrade for it that never completes the
    // probe handshake while keeping the connection open (mimics a proxy that forwards the upgrade
    // as a pooled request and never relays the `101`). The heartbeat is paused while upgrading, so
    // without an upgrade timeout the session — and its underlying connection — would leak forever.
    let sid = create_polling_connection(&mut svc).await.parse().unwrap();
    let _stream = create_ws_upgrade_connection(&mut svc, sid).await;

    let data = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("session was not reclaimed after an abandoned ws upgrade (leak)")
        .unwrap();

    assert_eq!(data, DisconnectReason::TransportError);
}

#[tokio::test]
pub async fn ws_upgrade_abandoned_parked_heartbeat_timeout() {
    let (disconnect_tx, mut rx) = mpsc::channel(10);
    // Long heartbeat so that, once parked between pings, it cannot reclaim the session on its own
    // within the test window; short upgrade timeout so the fix reclaims quickly.
    let config = EngineIoConfig::builder()
        .ping_interval(Duration::from_secs(30))
        .ping_timeout(Duration::from_secs(30))
        .upgrade_timeout(Duration::from_millis(200))
        .build();
    let mut svc = EngineIoService::with_config(Arc::new(MyHandler { disconnect_tx }), config);

    let sid = create_polling_connection(&mut svc).await;
    send_req(
        &mut svc,
        format!("transport=polling&sid={sid}"),
        http::Method::GET,
        None,
    )
    .await; // drain the first ping; the heartbeat is now waiting for the pong.
    send_req(
        &mut svc,
        format!("transport=polling&sid={sid}"),
        http::Method::POST,
        Some("3".into()),
    )
    .await; // pong — the heartbeat consumes it and parks between pings.
    let _stream = create_ws_upgrade_connection(&mut svc, sid.parse().unwrap()).await;

    let data = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("parked-heartbeat abandoned ws upgrade was not reclaimed (permanent leak)")
        .unwrap();

    assert_eq!(data, DisconnectReason::TransportError);
}

#[tokio::test]
pub async fn polling_transport_closed() {
    let (disconnect_tx, mut rx) = mpsc::channel(10);
    let mut svc = create_server(MyHandler { disconnect_tx }).await;
    let sid = create_polling_connection(&mut svc).await;

    send_req(
        &mut svc,
        format!("transport=polling&sid={sid}"),
        http::Method::POST,
        Some("1".into()),
    )
    .await;

    let data = tokio::time::timeout(Duration::from_millis(10), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::TransportClose")
        .unwrap();

    assert_eq!(data, DisconnectReason::TransportClose);
}

#[tokio::test]
pub async fn ws_transport_closed() {
    let (disconnect_tx, mut rx) = mpsc::channel(10);
    let mut svc = create_server(MyHandler { disconnect_tx }).await;
    let mut stream = create_ws_connection(&mut svc).await;

    stream.send(Message::Text("1".into())).await.unwrap();

    let data = tokio::time::timeout(Duration::from_millis(1), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::TransportClose")
        .unwrap();

    assert_eq!(data, DisconnectReason::TransportClose);
}

#[tokio::test]
pub async fn multiple_http_polling() {
    let (disconnect_tx, mut rx) = mpsc::channel(10);
    let mut svc = create_server(MyHandler { disconnect_tx }).await;
    let sid = create_polling_connection(&mut svc).await;
    send_req(
        &mut svc,
        format!("transport=polling&sid={sid}"),
        http::Method::GET,
        None,
    )
    .await; // we eat the first ping from the server.

    tokio::spawn(futures_util::future::join_all(vec![
        send_req(
            &mut svc,
            format!("transport=polling&sid={sid}"),
            http::Method::GET,
            None,
        ),
        send_req(
            &mut svc,
            format!("transport=polling&sid={sid}"),
            http::Method::GET,
            None,
        ),
    ]));

    let data = tokio::time::timeout(Duration::from_millis(10), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::DisconnectError::MultipleHttpPolling")
        .unwrap();

    assert_eq!(data, DisconnectReason::MultipleHttpPollingError);
}

#[tokio::test]
pub async fn polling_packet_parsing() {
    let (disconnect_tx, mut rx) = mpsc::channel(10);
    let mut svc = create_server(MyHandler { disconnect_tx }).await;
    let sid = create_polling_connection(&mut svc).await;
    send_req(
        &mut svc,
        format!("transport=polling&sid={sid}"),
        http::Method::POST,
        Some("aizdunazidaubdiz".into()),
    )
    .await;

    let data = tokio::time::timeout(Duration::from_millis(1), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::PacketParsingError")
        .unwrap();

    assert_eq!(data, DisconnectReason::PacketParsingError);
}

#[tokio::test]
pub async fn ws_packet_parsing() {
    let (disconnect_tx, mut rx) = mpsc::channel(10);
    let mut svc = create_server(MyHandler { disconnect_tx }).await;
    let mut stream = create_ws_connection(&mut svc).await;
    stream
        .send(Message::Text("aizdunazidaubdiz".into()))
        .await
        .unwrap();

    let data = tokio::time::timeout(Duration::from_millis(1), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::TransportError::PacketParsing")
        .unwrap();

    assert_eq!(data, DisconnectReason::PacketParsingError);
}
