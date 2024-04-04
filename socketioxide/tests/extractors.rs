//! Tests for extractors
use std::time::Duration;

use serde_json::json;
use socketioxide::extract::{Data, SocketRef, State, TryData};
use tokio::sync::mpsc;

use engineioxide::Packet as EioPacket;
use socketioxide::packet::Packet;
use socketioxide::SocketIo;
mod fixture;
mod utils;

async fn timeout_rcv<T: std::fmt::Debug>(srx: &mut tokio::sync::mpsc::Receiver<T>) -> T {
    tokio::time::timeout(Duration::from_millis(200), srx.recv())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
pub async fn state_extractor() {
    let state = 1112i32;
    let (_, io) = SocketIo::builder().with_state(state).build_svc();

    io.ns("/", |socket: SocketRef, State(state): State<i32>| {
        assert_ok!(socket.emit("state", state));
        socket.on("test", |socket: SocketRef, State(state): State<i32>| {
            assert_ok!(socket.emit("state", state));
        });
    });
    let res_packet = EioPacket::Message(Packet::event("/", "state", state.into()).into());

    // Connect packet
    let (stx, mut srx) = io.new_dummy_sock("/", ()).await;
    srx.recv().await;

    // First echoed res packet from connect handler
    assert_eq!(timeout_rcv(&mut srx).await, res_packet);

    let packet = EioPacket::Message(Packet::event("/", "test", json!("foo")).into());
    assert_ok!(stx.try_send(packet));

    // second echoed res packet from test event handler
    assert_eq!(timeout_rcv(&mut srx).await, res_packet);
}

#[tokio::test]
pub async fn data_extractor() {
    let (_, io) = SocketIo::new_svc();
    let (tx, mut rx) = mpsc::channel::<String>(4);
    let tx1 = tx.clone();

    io.ns("/", move |socket: SocketRef, Data(data): Data<String>| {
        assert_ok!(tx.try_send(data));
        socket.on("test", move |Data(data): Data<String>| {
            assert_ok!(tx.try_send(data));
        });
    });

    io.new_dummy_sock("/", ()).await;
    assert!(matches!(
        rx.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    io.new_dummy_sock("/", 1321).await;
    assert!(matches!(
        rx.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));

    // Capacity should be the same as the handler should not be called
    assert_eq!(tx1.capacity(), 4);

    let (stx, _rtx) = io.new_dummy_sock("/", "foo").await;
    assert_eq!(timeout_rcv(&mut rx).await, "foo");

    let packet = EioPacket::Message(Packet::event("/", "test", json!("oof")).into());
    assert_ok!(stx.try_send(packet));
    assert_eq!(timeout_rcv(&mut rx).await, "oof");

    let packet = EioPacket::Message(Packet::event("/", "test", json!({ "test": 132 })).into());
    assert_ok!(stx.try_send(packet));
    // Capacity should be the same as the handler should not be called
    assert_eq!(tx1.capacity(), 4);
}

#[tokio::test]
pub async fn try_data_extractor() {
    let (_, io) = SocketIo::new_svc();
    let (tx, mut rx) = mpsc::channel::<Result<String, serde_json::Error>>(4);
    io.ns("/", move |s: SocketRef, TryData(data): TryData<String>| {
        assert_ok!(tx.try_send(data));
        s.on("test", move |TryData(data): TryData<String>| {
            assert_ok!(tx.try_send(data));
        });
    });

    // Non deserializable data
    io.new_dummy_sock("/", ()).await;
    assert_err!(timeout_rcv(&mut rx).await);

    // Non deserializable data
    io.new_dummy_sock("/", 1321).await;
    assert_err!(timeout_rcv(&mut rx).await);

    let (stx, _rtx) = io.new_dummy_sock("/", "foo").await;
    let res = assert_ok!(timeout_rcv(&mut rx).await);
    assert_eq!(res, "foo");

    let packet = EioPacket::Message(Packet::event("/", "test", json!("oof")).into());
    assert_ok!(stx.try_send(packet));
    let res = assert_ok!(timeout_rcv(&mut rx).await);
    assert_eq!(res, "oof");

    // Non deserializable data
    let packet = EioPacket::Message(Packet::event("/", "test", json!({ "test": 132 })).into());
    assert_ok!(stx.try_send(packet));
    assert_err!(timeout_rcv(&mut rx).await);
}
