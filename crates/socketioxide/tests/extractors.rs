//! Tests for extractors
use std::convert::Infallible;
use std::time::Duration;

use serde_json::json;
use socketioxide::ParserError;
use socketioxide::extract::{Data, Extension, MaybeExtension, SocketRef, State, TryData};
use socketioxide::handler::ConnectHandler;
use socketioxide_core::Value;
use socketioxide_core::parser::Parse;
use socketioxide_parser_common::CommonParser;
use tokio::sync::mpsc;

use engineioxide::Packet as EioPacket;
use socketioxide::SocketIo;
use socketioxide_core::packet::Packet;
mod fixture;
mod utils;

async fn timeout_rcv<T: std::fmt::Debug>(srx: &mut tokio::sync::mpsc::Receiver<T>) -> T {
    tokio::time::timeout(Duration::from_millis(10), srx.recv())
        .await
        .unwrap()
        .unwrap()
}
async fn timeout_rcv_err<T: std::fmt::Debug>(srx: &mut tokio::sync::mpsc::Receiver<T>) {
    tokio::time::timeout(Duration::from_millis(10), srx.recv())
        .await
        .unwrap_err();
}

fn create_msg(ns: &'static str, event: &str, data: impl serde::Serialize) -> EioPacket {
    let val = CommonParser.encode_value(&data, Some(event)).unwrap();
    match CommonParser.encode(Packet::event(ns, val)) {
        Value::Str(msg, _) => EioPacket::Message(msg),
        Value::Bytes(_) => unreachable!(),
    }
}

#[tokio::test]
pub async fn state_extractor() {
    let state = 1112i32;
    let (_, io) = SocketIo::builder().with_state(state).build_svc();

    io.ns("/", |socket: SocketRef, State(state): State<i32>| {
        assert_ok!(socket.emit("state", &state));
        socket.on("test", |socket: SocketRef, State(state): State<i32>| {
            assert_ok!(socket.emit("state", &state));
        });
    });
    let res_packet = create_msg("/", "state", state);

    // Connect packet
    let (stx, mut srx) = io.new_dummy_sock("/", ()).await;
    srx.recv().await;

    // First echoed res packet from connect handler
    assert_eq!(timeout_rcv(&mut srx).await, res_packet);

    assert_ok!(stx.try_send(create_msg("/", "test", "foo")));

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

    assert_ok!(stx.try_send(create_msg("/", "test", "oof")));
    assert_eq!(timeout_rcv(&mut rx).await, "oof");

    assert_ok!(stx.try_send(create_msg("/", "test", json!({ "test": 132 }))));
    // Capacity should be the same as the handler should not be called
    assert_eq!(tx1.capacity(), 4);
}

#[tokio::test]
pub async fn try_data_extractor() {
    let (_, io) = SocketIo::new_svc();
    let (tx, mut rx) = mpsc::channel::<Result<String, ParserError>>(4);
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

    assert_ok!(stx.try_send(create_msg("/", "test", "oof")));
    let res = assert_ok!(timeout_rcv(&mut rx).await);
    assert_eq!(res, "oof");

    // Non deserializable data
    assert_ok!(stx.try_send(create_msg("/", "test", json!({ "test": 132 }))));
    assert_err!(timeout_rcv(&mut rx).await);
}

#[tokio::test]
pub async fn extension_extractor() {
    let (_, io) = SocketIo::new_svc();

    fn on_test(s: SocketRef, Extension(i): Extension<usize>) {
        s.emit("from_ev_test", &i).unwrap();
    }
    fn ns_root(s: SocketRef, Extension(i): Extension<usize>) {
        s.emit("from_ns", &i).unwrap();
        s.on("test", on_test);
    }
    fn set_ext(s: SocketRef) -> Result<(), Infallible> {
        s.extensions.insert(123usize);
        Ok(())
    }

    // Namespace without errors (the extension is set)
    io.ns("/", ns_root.with(set_ext));
    // Namespace with errors (the extension is not set)
    io.ns("/test", ns_root);

    // Extract extensions from the socket
    let (tx, mut rx) = io.new_dummy_sock("/", ()).await;
    assert!(matches!(timeout_rcv(&mut rx).await, EioPacket::Message(s) if s.starts_with('0')));
    assert_eq!(
        timeout_rcv(&mut rx).await,
        EioPacket::Message("2[\"from_ns\",123]".into())
    );
    assert_ok!(tx.try_send(create_msg("/", "test", ())));
    assert_eq!(
        timeout_rcv(&mut rx).await,
        EioPacket::Message("2[\"from_ev_test\",123]".into())
    );

    // Extract unknown extensions from the socket
    let (tx, mut rx) = io.new_dummy_sock("/test", ()).await;
    assert!(matches!(timeout_rcv(&mut rx).await, EioPacket::Message(s) if s.starts_with('0')));
    timeout_rcv_err(&mut rx).await;
    assert_ok!(tx.try_send(create_msg("/test", "test", ())));
    timeout_rcv_err(&mut rx).await;
}

#[tokio::test]
pub async fn maybe_extension_extractor() {
    let (_, io) = SocketIo::new_svc();

    fn on_test(s: SocketRef, MaybeExtension(i): MaybeExtension<usize>) {
        s.emit("from_ev_test", &i).unwrap();
    }
    fn ns_root(s: SocketRef, MaybeExtension(i): MaybeExtension<usize>) {
        s.emit("from_ns", &i).unwrap();
        s.on("test", on_test);
    }
    fn set_ext(s: SocketRef) -> Result<(), Infallible> {
        s.extensions.insert(123usize);
        Ok(())
    }

    // Namespace without errors (the extension is set)
    io.ns("/", ns_root.with(set_ext));
    // Namespace with errors (the extension is not set)
    io.ns("/test", ns_root);

    // Extract extensions from the socket
    let (tx, mut rx) = io.new_dummy_sock("/", ()).await;
    assert!(matches!(timeout_rcv(&mut rx).await, EioPacket::Message(s) if s.starts_with('0')));
    assert_eq!(
        timeout_rcv(&mut rx).await,
        EioPacket::Message("2[\"from_ns\",123]".into())
    );
    assert_ok!(tx.try_send(create_msg("/", "test", ())));
    assert_eq!(
        timeout_rcv(&mut rx).await,
        EioPacket::Message("2[\"from_ev_test\",123]".into())
    );

    // Extract unknown extensions from the socket
    let (tx, mut rx) = io.new_dummy_sock("/test", ()).await;
    assert!(matches!(timeout_rcv(&mut rx).await, EioPacket::Message(s) if s.starts_with('0')));
    assert_eq!(
        timeout_rcv(&mut rx).await,
        EioPacket::Message("2/test,[\"from_ns\",null]".into())
    );
    assert_ok!(tx.try_send(create_msg("/test", "test", ())));
    assert_eq!(
        timeout_rcv(&mut rx).await,
        EioPacket::Message("2/test,[\"from_ev_test\",null]".into())
    );
}
