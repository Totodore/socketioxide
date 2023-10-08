//! Tests for disconnect reasons
//! Test are made on polling and websocket transports for engine.io errors and only websocket for socket.io errors:
//! * Heartbeat timeout
//! * Transport close
//! * Multiple http polling
//! * Packet parsing
//!
//! * Client namespace disconnect
//! * Server namespace disconnect

use std::time::Duration;

use futures::{SinkExt, StreamExt};
use socketioxide::{DisconnectReason, SocketIo, SocketIoBuilder};
use tokio::sync::mpsc;

mod fixture;

use fixture::{create_server, send_req};
use tokio_tungstenite::tungstenite::Message;

use crate::fixture::{create_polling_connection, create_ws_connection};

fn create_handler(chan_size: usize) -> (SocketIoBuilder, mpsc::Receiver<DisconnectReason>) {
    let (tx, rx) = mpsc::channel::<DisconnectReason>(chan_size);
    let builder = SocketIo::builder().ns("/", move |socket| {
        println!("Socket connected on / namespace with id: {}", socket.sid);
        let tx = tx.clone();
        socket.on_disconnect(move |socket, reason| {
            println!("Socket.IO disconnected: {} {}", socket.sid, reason);
            tx.try_send(reason).unwrap();
            async move {}
        });

        async move {}
    });
    (builder, rx)
}

// Engine IO Disconnect Reason Tests

#[tokio::test]
pub async fn polling_heartbeat_timeout() {
    let (ns, mut rx) = create_handler(1);
    create_server(ns, 1234);
    create_polling_connection(1234).await;

    let data = tokio::time::timeout(Duration::from_millis(500), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::HeartbeatTimeout")
        .unwrap();

    assert_eq!(data, DisconnectReason::HeartbeatTimeout);
}

#[tokio::test]
pub async fn ws_heartbeat_timeout() {
    let (ns, mut rx) = create_handler(1);
    create_server(ns, 12344);
    let _stream = create_ws_connection(12344).await;

    let data = tokio::time::timeout(Duration::from_millis(500), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::HeartbeatTimeout")
        .unwrap();

    assert_eq!(data, DisconnectReason::HeartbeatTimeout);
}

#[tokio::test]
pub async fn polling_transport_closed() {
    let (ns, mut rx) = create_handler(1);
    create_server(ns, 1235);
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
    let (ns, mut rx) = create_handler(1);
    create_server(ns, 12345);
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
    let (ns, mut rx) = create_handler(1);
    create_server(ns, 1236);
    let sid = create_polling_connection(1236).await;

    // First request to flush the server buffer containing the open packet
    send_req(
        1236,
        format!("transport=polling&sid={sid}"),
        http::Method::GET,
        None,
    )
    .await;

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
    let (ns, mut rx) = create_handler(1);
    create_server(ns, 1237);
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
    let (ns, mut rx) = create_handler(1);
    create_server(ns, 12347);
    let mut stream = create_ws_connection(12347).await;
    stream
        .send(Message::Text("aizdunazidaubdiz".into()))
        .await
        .unwrap();

    let data = tokio::time::timeout(Duration::from_millis(1), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::PacketParsingError")
        .unwrap();

    assert_eq!(data, DisconnectReason::PacketParsingError);
}

// Socket IO Disconnect Reason Tests

#[tokio::test]
pub async fn client_ns_disconnect() {
    let (ns, mut rx) = create_handler(1);
    create_server(ns, 12348);
    let mut stream = create_ws_connection(12348).await;

    stream.send(Message::Text("41".into())).await.unwrap();

    let data = tokio::time::timeout(Duration::from_millis(1), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::ClientNSDisconnect")
        .unwrap();

    assert_eq!(data, DisconnectReason::ClientNSDisconnect);
}

#[tokio::test]
pub async fn server_ns_disconnect() {
    let (tx, mut rx) = mpsc::channel::<DisconnectReason>(1);
    let builder = SocketIo::builder().ns("/", move |socket| {
        println!("Socket connected on / namespace with id: {}", socket.sid);
        let sock = socket.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            sock.disconnect().unwrap();
        });

        socket.on_disconnect(move |socket, reason| {
            println!("Socket.IO disconnected: {} {}", socket.sid, reason);
            tx.try_send(reason).unwrap();
            async move {}
        });

        async move {}
    });

    create_server(builder, 12349);
    let _stream = create_ws_connection(12349).await;

    let data = tokio::time::timeout(Duration::from_millis(20), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::ServerNSDisconnect")
        .unwrap();
    assert_eq!(data, DisconnectReason::ServerNSDisconnect);
}

#[tokio::test]
pub async fn server_http_closing() {
    let (builder, _rx) = create_handler(100);

    let io = create_server(builder, 12350);
    let mut streams =
        futures::future::join_all((0..100).map(|_| create_ws_connection(12350))).await;
    futures::future::join_all(streams.iter_mut().map(|s| async move {
        s.next().await; // engine.io open packet
        s.next().await; // socket.io open packet
    }))
    .await;

    tokio::time::timeout(Duration::from_millis(2000), io.close())
        .await
        .expect("timeout waiting for server closing");
    let packets = futures::future::join_all(streams.iter_mut().map(|s| async move {
        (s.next().await, s.next().await) // Closing packet / None
    }))
    .await;
    for packet in packets {
        println!("{:?}", packet);
        assert!(matches!(packet.0.unwrap().unwrap(), Message::Close(_)));
        assert!(packet.1.is_none());
    }
}
