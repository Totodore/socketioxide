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
    handler::EngineIoHandler,
    socket::{DisconnectReason, Socket},
};
use futures_util::SinkExt;
use tokio::sync::mpsc;

mod fixture;

use fixture::{create_server, create_ws_connection, send_req};
use tokio_tungstenite::tungstenite::Message;

use crate::fixture::create_polling_connection;

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
