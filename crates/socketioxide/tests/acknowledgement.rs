//! Tests for acknowledgements
mod utils;

use engineioxide::Packet::*;
use futures_util::StreamExt;
use socketioxide::extract::SocketRef;
use socketioxide::packet::PacketData;
use socketioxide::SocketIo;
use socketioxide_core::parser::Parse;
use socketioxide_parser_common::CommonParser;
use tokio::sync::mpsc;
use tokio::time::Duration;

#[tokio::test]
pub async fn emit_with_ack() {
    let (_svc, io) = SocketIo::new_svc();
    let (tx, mut rx) = mpsc::channel::<[String; 1]>(4);

    io.ns("/", move |s: SocketRef| async move {
        let res = assert_ok!(s.emit_with_ack::<_, [String; 1]>("test", "foo")).await;
        let ack = assert_ok!(res);
        assert_ok!(tx.try_send(ack));

        let res = s
            .timeout(Duration::from_millis(500))
            .emit_with_ack::<_, [String; 1]>("test", &"foo");
        let res = assert_ok!(res).await;
        let ack = assert_ok!(res);
        assert_ok!(tx.try_send(ack));
    });

    let (stx, mut srx) = io.new_dummy_sock("/", ()).await;
    assert_some!(srx.recv().await); // NS connect packet

    let msg = assert_some!(srx.recv().await);
    assert_eq!(msg, Message("21[\"test\",\"foo\"]".into()));
    assert_ok!(stx.send(Message("31[\"oof\"]".into())).await);

    let ack = rx.recv().await.unwrap();
    assert_eq!(ack[0], "oof");

    let msg = assert_some!(srx.recv().await);
    assert_eq!(msg, Message("22[\"test\",\"foo\"]".into()));
    assert_ok!(stx.send(Message("32[\"oof\"]".into())).await);

    let ack = rx.recv().await.unwrap();
    assert_eq!(ack[0], "oof");
}

#[tokio::test]
pub async fn broadcast_with_ack() {
    let (_svc, io) = SocketIo::new_svc();
    let (tx, mut rx) = mpsc::channel::<[String; 1]>(100);

    io.ns("/", move |socket: SocketRef, io: SocketIo| async move {
        let res = io.emit_with_ack::<_, [String; 1]>("test", "foo").await;
        let res = assert_ok!(res);
        res.for_each(|(id, res)| {
            let ack = assert_ok!(res);
            assert_ok!(tx.try_send(ack));
            assert_some!(io.sockets().iter().find(|s| s.id == id));
            async move {}
        })
        .await;

        let res = io
            .timeout(Duration::from_millis(500))
            .emit_with_ack::<_, [String; 1]>("test", "foo")
            .await;
        let res = assert_ok!(res);
        res.for_each(|(id, res)| {
            let ack = assert_ok!(res);
            assert_ok!(tx.try_send(ack));
            assert_some!(io.sockets().iter().find(|s| s.id == id));
            async move {}
        })
        .await;

        let res = socket
            .broadcast()
            .timeout(Duration::from_millis(500))
            .emit_with_ack::<_, [String; 1]>("test", "foo")
            .await;
        let res = assert_ok!(res);
        res.for_each(|(id, res)| {
            let ack = assert_ok!(res);
            assert_ok!(tx.try_send(ack));
            assert_some!(io.sockets().iter().find(|s| s.id == id));
            async move {}
        })
        .await;
    });

    // Spawn 5 clients and make them echo the ack
    for _ in 0..5 {
        let io = io.clone();
        tokio::spawn(async move {
            let (stx, mut srx) = io.new_dummy_sock("/", ()).await;
            assert_some!(srx.recv().await);
            assert_some!(srx.recv().await);
            let parser = CommonParser;
            while let Some(msg) = srx.recv().await {
                let msg = match msg {
                    Message(msg) => msg,
                    msg => panic!("Unexpected message: {:?}", msg),
                };
                let ack = match assert_ok!(parser.decode_str(&Default::default(), msg)).inner {
                    PacketData::Event(_, Some(ack)) => ack,
                    _ => panic!("Unexpected packet"),
                };
                assert_ok!(stx.send(Message(format!("3{}[\"oof\"]", ack).into())).await);
            }
        });
    }

    for _ in 0..5 {
        for _ in 0..3 {
            let msg = rx.recv().await.unwrap();
            assert_eq!(msg[0], "oof");
        }
    }
}
