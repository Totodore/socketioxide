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
    io.ns("/", move |s: SocketRef| {
        s.on_disconnect(move |r: DisconnectReason| tx.try_send(r).unwrap())
    });
    rx
}

// Engine IO Disconnect Reason Tests

#[tokio::test]
pub async fn polling_heartbeat_timeout() {
    let (svc, io) = create_server().await;
    let mut rx = attach_handler(&io, 1);
    create_polling_connection(&svc).await;

    let data = tokio::time::timeout(Duration::from_millis(500), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::HeartbeatTimeout")
        .unwrap();

    assert_eq!(data, DisconnectReason::HeartbeatTimeout);
}

#[tokio::test]
pub async fn ws_heartbeat_timeout() {
    let (svc, io) = create_server().await;
    let mut rx = attach_handler(&io, 1);
    let _stream = create_ws_connection(&svc).await;

    let data = tokio::time::timeout(Duration::from_millis(500), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::HeartbeatTimeout")
        .unwrap();

    assert_eq!(data, DisconnectReason::HeartbeatTimeout);
}

#[tokio::test]
pub async fn polling_transport_closed() {
    let (svc, io) = create_server().await;
    let mut rx = attach_handler(&io, 1);
    let sid = create_polling_connection(&svc).await;

    send_req(
        &svc,
        format!("transport=polling&sid={sid}"),
        http::Method::POST,
        Some("1".into()),
    )
    .await;

    let data = tokio::time::timeout(Duration::from_millis(100), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::TransportClose")
        .unwrap();

    assert_eq!(data, DisconnectReason::TransportClose);
}

#[tokio::test]
pub async fn ws_transport_closed() {
    let (svc, io) = create_server().await;
    let mut rx = attach_handler(&io, 1);
    let mut stream = create_ws_connection(&svc).await;

    stream.send(Message::Text("1".into())).await.unwrap();

    let data = tokio::time::timeout(Duration::from_millis(100), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::TransportClose")
        .unwrap();

    assert_eq!(data, DisconnectReason::TransportClose);
}

#[tokio::test]
pub async fn multiple_http_polling() {
    let (svc, io) = create_server().await;
    let mut rx = attach_handler(&io, 1);
    let sid = create_polling_connection(&svc).await;

    // First request to flush the server buffer containing the open packet
    send_req(
        &svc,
        format!("transport=polling&sid={sid}"),
        http::Method::GET,
        None,
    )
    .await;

    let (_, _, data) = futures_util::future::join3(
        send_req(
            &svc,
            format!("transport=polling&sid={sid}"),
            http::Method::GET,
            None,
        ),
        send_req(
            &svc,
            format!("transport=polling&sid={sid}"),
            http::Method::GET,
            None,
        ),
        async move {
            tokio::time::timeout(Duration::from_millis(100), rx.recv())
                .await
                .expect(
                    "timeout waiting for DisconnectReason::DisconnectError::MultipleHttpPolling",
                )
                .unwrap()
        },
    )
    .await;

    assert_eq!(data, DisconnectReason::MultipleHttpPollingError);
}

#[tokio::test]
pub async fn polling_packet_parsing() {
    let (svc, io) = create_server().await;
    let mut rx = attach_handler(&io, 1);
    let sid = create_polling_connection(&svc).await;
    send_req(
        &svc,
        format!("transport=polling&sid={sid}"),
        http::Method::POST,
        Some("aizdunazidaubdiz".into()),
    )
    .await;

    let data = tokio::time::timeout(Duration::from_millis(100), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::PacketParsingError")
        .unwrap();

    assert_eq!(data, DisconnectReason::PacketParsingError);
}

#[tokio::test]
pub async fn ws_packet_parsing() {
    let (svc, io) = create_server().await;
    let mut rx = attach_handler(&io, 1);
    let mut stream = create_ws_connection(&svc).await;
    stream
        .send(Message::Text("aizdunazidaubdiz".into()))
        .await
        .unwrap();

    let data = tokio::time::timeout(Duration::from_millis(100), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::PacketParsingError")
        .unwrap();

    assert_eq!(data, DisconnectReason::PacketParsingError);
}

// Socket IO Disconnect Reason Tests

#[tokio::test]
pub async fn client_ns_disconnect() {
    let (svc, io) = create_server().await;
    let mut rx = attach_handler(&io, 1);
    let mut stream = create_ws_connection(&svc).await;

    stream.send(Message::Text("41".into())).await.unwrap();

    let data = tokio::time::timeout(Duration::from_millis(100), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::ClientNSDisconnect")
        .unwrap();

    assert_eq!(data, DisconnectReason::ClientNSDisconnect);
}

#[tokio::test]
pub async fn server_ns_disconnect() {
    let (tx, mut rx) = mpsc::channel::<DisconnectReason>(1);
    let (svc, io) = create_server().await;
    io.ns("/", move |socket: SocketRef, io: SocketIo| {
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let s = io.sockets().into_iter().next().unwrap();
            s.disconnect().unwrap();
        });

        socket.on_disconnect(move |reason: DisconnectReason| tx.try_send(reason).unwrap());
    });

    let _stream = create_ws_connection(&svc).await;

    let data = tokio::time::timeout(Duration::from_millis(100), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::ServerNSDisconnect")
        .unwrap();
    assert_eq!(data, DisconnectReason::ServerNSDisconnect);
}

#[tokio::test]
pub async fn server_ns_close() {
    let (tx, mut rx) = mpsc::channel::<DisconnectReason>(1);
    let (svc, io) = create_server().await;
    io.ns("/test", move |socket: SocketRef, io: SocketIo| {
        socket.on_disconnect(move |reason: DisconnectReason| tx.try_send(reason).unwrap());
        io.delete_ns("/test");
    });

    let mut ws = create_ws_connection(&svc).await;
    ws.send(Message::Text("40/test,{}".into())).await.unwrap();
    let data = tokio::time::timeout(Duration::from_millis(20), rx.recv())
        .await
        .expect("timeout waiting for DisconnectReason::ServerNSDisconnect")
        .unwrap();
    assert_eq!(data, DisconnectReason::ServerNSDisconnect);
}

#[tokio::test]
pub async fn server_ws_closing() {
    let (svc, io) = create_server().await;
    let _rx = attach_handler(&io, 100);

    let mut streams =
        futures_util::future::join_all((0..100).map(|_| create_ws_connection(&svc))).await;
    futures_util::future::join_all(streams.iter_mut().map(|s| async move {
        s.next().await; // engine.io open packet
        s.next().await; // socket.io open packet
        s.next().await; // engine.io ping packet
    }))
    .await;

    tokio::time::timeout(Duration::from_millis(100), io.close())
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
    let (svc, io) = create_server().await;
    let _rx = attach_handler(&io, 100);
    let mut sids =
        futures_util::future::join_all((0..100).map(|_| create_polling_connection(&svc))).await;
    futures_util::future::join_all(sids.iter_mut().map(|s| {
        send_req(
            &svc,
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
            &svc,
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
