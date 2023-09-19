//! Tests for disconnect reasons
//! Test are made on polling and websocket transports:
//! * Heartbeat timeout
//! * Transport close
//! * Multiple http polling
//! * Packet parsing

use std::time::Duration;

use engineioxide::{
    handler::EngineIoHandler,
    socket::{DisconnectReason, Socket},
};
use futures::SinkExt;
use tokio::sync::mpsc;

mod fixture;

use fixture::{create_server, send_req};
use tokio_tungstenite::tungstenite::Message;

use crate::fixture::{create_polling_connection, create_ws_connection};

#[derive(Debug, Clone)]
struct MyHandler {
    disconnect_tx: mpsc::Sender<DisconnectReason>,
}

#[engineioxide::async_trait]
impl EngineIoHandler for MyHandler {
    type Data = ();

    fn on_connect(&self, socket: &Socket<Self>) {
        println!("socket connect {}", socket.sid);
    }
    fn on_disconnect(&self, socket: &Socket<Self>, reason: DisconnectReason) {
        println!("socket disconnect {}: {:?}", socket.sid, reason);
        self.disconnect_tx.try_send(reason).unwrap();
    }

    fn on_message(&self, msg: String, socket: &Socket<Self>) {
        println!("Ping pong message {:?}", msg);
        socket.emit(msg).ok();
    }

    fn on_binary(&self, data: Vec<u8>, socket: &Socket<Self>) {
        println!("Ping pong binary message {:?}", data);
        socket.emit_binary(data).ok();
    }
}

#[tokio::test]
pub async fn polling_heartbeat_timeout() {
    let (disconnect_tx, mut rx) = mpsc::channel(10);
    create_server(MyHandler { disconnect_tx }, 1234);
    create_polling_connection(1234).await;

    let data = tokio::time::timeout(Duration::from_millis(500), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::HeartbeatTimeout")
        .unwrap();

    assert_eq!(data, DisconnectReason::HeartbeatTimeout);
}

#[tokio::test]
pub async fn ws_heartbeat_timeout() {
    let (disconnect_tx, mut rx) = mpsc::channel(10);
    create_server(MyHandler { disconnect_tx }, 12344);
    let _stream = create_ws_connection(12344).await;

    let data = tokio::time::timeout(Duration::from_millis(500), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::HeartbeatTimeout")
        .unwrap();

    assert_eq!(data, DisconnectReason::HeartbeatTimeout);
}

#[tokio::test]
pub async fn polling_transport_closed() {
    let (disconnect_tx, mut rx) = mpsc::channel(10);
    create_server(MyHandler { disconnect_tx }, 1235);
    let sid = create_polling_connection(1235).await;

    send_req(
        1235,
        format!("transport=polling&sid={sid}"),
        http::Method::POST,
        Some("1".into()),
    )
    .await;

    let data = tokio::time::timeout(Duration::from_millis(1), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::TransportClose")
        .unwrap();

    assert_eq!(data, DisconnectReason::TransportClose);
}

#[tokio::test]
pub async fn ws_transport_closed() {
    let (disconnect_tx, mut rx) = mpsc::channel(10);
    create_server(MyHandler { disconnect_tx }, 12345);
    let mut stream = create_ws_connection(12345).await;

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
    create_server(MyHandler { disconnect_tx }, 1236);
    let sid = create_polling_connection(1236).await;

    tokio::spawn(futures::future::join_all(vec![
        send_req(
            1236,
            format!("transport=polling&sid={sid}"),
            http::Method::GET,
            None,
        ),
        send_req(
            1236,
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
    create_server(MyHandler { disconnect_tx }, 1237);
    let sid = create_polling_connection(1237).await;
    send_req(
        1237,
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
    create_server(MyHandler { disconnect_tx }, 12347);
    let mut stream = create_ws_connection(12347).await;
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
