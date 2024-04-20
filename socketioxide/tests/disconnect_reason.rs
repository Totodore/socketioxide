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

use futures_util::{SinkExt, StreamExt};
use socketioxide::{extract::SocketRef, socket::DisconnectReason, SocketIo};
use tokio::sync::mpsc;

mod fixture;

use fixture::{create_server, send_req};
use tokio_tungstenite::tungstenite::Message;

use crate::fixture::{create_polling_connection, create_ws_connection};

fn attach_handler(io: &SocketIo, chan_size: usize) -> mpsc::Receiver<DisconnectReason> {
    let (tx, rx) = mpsc::channel::<DisconnectReason>(chan_size);
    io.ns("/", move |socket: SocketRef| {
        println!("Socket connected on / namespace with id: {}", socket.id);
        let tx = tx.clone();
        socket.on_disconnect(move |s: SocketRef, reason: DisconnectReason| {
            println!("Socket.IO disconnected: {} {}", s.id, reason);
            tx.try_send(reason).unwrap();
        });
    });
    rx
}

// Engine IO Disconnect Reason Tests

#[tokio::test]
pub async fn polling_heartbeat_timeout() {
    let io = create_server(1234).await;
    let mut rx = attach_handler(&io, 1);
    create_polling_connection(1234).await;

    let data = tokio::time::timeout(Duration::from_millis(500), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::HeartbeatTimeout")
        .unwrap();

    assert_eq!(data, DisconnectReason::HeartbeatTimeout);
}

#[tokio::test]
pub async fn ws_heartbeat_timeout() {
    let io = create_server(12344).await;
    let mut rx = attach_handler(&io, 1);
    let _stream = create_ws_connection(12344).await;

    let data = tokio::time::timeout(Duration::from_millis(500), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::HeartbeatTimeout")
        .unwrap();

    assert_eq!(data, DisconnectReason::HeartbeatTimeout);
}

#[tokio::test]
pub async fn polling_transport_closed() {
    let io = create_server(1235).await;
    let mut rx = attach_handler(&io, 1);
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
    let io = create_server(12345).await;
    let mut rx = attach_handler(&io, 1);
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
    let io = create_server(1236).await;
    let mut rx = attach_handler(&io, 1);
    let sid = create_polling_connection(1236).await;

    // First request to flush the server buffer containing the open packet
    send_req(
        1236,
        format!("transport=polling&sid={sid}"),
        http::Method::GET,
        None,
    )
    .await;

    tokio::spawn(futures_util::future::join_all(vec![
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
    let io = create_server(1237).await;
    let mut rx = attach_handler(&io, 1);
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
    let io = create_server(12347).await;
    let mut rx = attach_handler(&io, 1);
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
    let io = create_server(12348).await;
    let mut rx = attach_handler(&io, 1);
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
    let io = create_server(12349).await;
    let io2 = io.clone();
    io.ns("/", move |socket: SocketRef| {
        let id = socket.id;
        println!("Socket connected on / namespace with id: {}", id);
        let tx = tx.clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            let s = io2.sockets().unwrap().into_iter().nth(0).unwrap();
            s.disconnect().unwrap();
        });

        socket.on_disconnect(move |socket: SocketRef, reason: DisconnectReason| {
            println!("Socket.IO disconnected: {} {}", socket.id, reason);
            tx.try_send(reason).unwrap();
        });
    });

    let _stream = create_ws_connection(12349).await;

    let data = tokio::time::timeout(Duration::from_millis(20), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::ServerNSDisconnect")
        .unwrap();
    assert_eq!(data, DisconnectReason::ServerNSDisconnect);
}

#[tokio::test]
pub async fn server_ws_closing() {
    let io = create_server(12350).await;
    let _rx = attach_handler(&io, 100);

    let mut streams =
        futures_util::future::join_all((0..100).map(|_| create_ws_connection(12350))).await;
    futures_util::future::join_all(streams.iter_mut().map(|s| async move {
        s.next().await; // engine.io open packet
        s.next().await; // socket.io open packet
    }))
    .await;

    tokio::time::timeout(Duration::from_millis(20), io.close())
        .await
        .expect("timeout waiting for server closing");
    let packets = futures_util::future::join_all(streams.iter_mut().map(|s| async move {
        (s.next().await, s.next().await) // Closing packet / None
    }))
    .await;
    for packet in packets {
        assert!(matches!(packet.0.unwrap().unwrap(), Message::Close(_)));
        assert!(packet.1.is_none());
    }
}

#[tokio::test]
pub async fn server_http_closing() {
    let io = create_server(12351).await;
    let _rx = attach_handler(&io, 100);
    let mut sids =
        futures_util::future::join_all((0..100).map(|_| create_polling_connection(12351))).await;
    futures_util::future::join_all(sids.iter_mut().map(|s| {
        send_req(
            12351,
            format!("transport=polling&sid={s}"),
            http::Method::GET,
            None,
        )
    }))
    .await;

    tokio::time::timeout(Duration::from_millis(20), io.close())
        .await
        .expect("timeout waiting for server closing");
    let packets = futures_util::future::join_all(sids.iter_mut().map(|s| {
        send_req(
            12351,
            format!("transport=polling&sid={s}"),
            http::Method::GET,
            None,
        )
    }))
    .await;
    for packet in packets {
        assert_eq!(
            packet,
            "{\"code\":\"1\",\"message\":\"Session ID unknown\"}"
        );
    }
}
