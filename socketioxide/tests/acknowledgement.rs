//! Tests for acknowledgements
//!
//! TODO: switch to `rust_socketio` when it will support ack responses
mod fixture;
mod utils;

use fixture::{create_server, create_ws_connection};
use futures::{SinkExt, StreamExt};
use socketioxide::extract::SocketRef;
use socketioxide::packet::{Packet, PacketData};
use tokio::sync::mpsc;
use tokio::time::Duration;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
pub async fn emit_with_ack() {
    const PORT: u16 = 2100;
    use Message::*;
    let io = create_server(PORT).await;
    let (tx, mut rx) = mpsc::channel::<[String; 1]>(4);

    io.ns("/", move |s: SocketRef| async move {
        let res = assert_ok!(s.emit_with_ack::<[String; 1]>("test", "foo")).await;
        let ack = assert_ok!(res);
        assert_ok!(tx.try_send(ack.data));

        let res = s
            .timeout(Duration::from_millis(500))
            .emit_with_ack::<_, [String; 1]>("test", "foo");
        let res = assert_ok!(res).await;
        let ack = assert_ok!(res);
        assert_ok!(tx.try_send(ack.data));
    });

    let (mut stx, mut srx) = create_ws_connection(PORT).await.split();
    assert_ok!(srx.next().await.unwrap());
    assert_ok!(srx.next().await.unwrap());

    let msg = assert_ok!(srx.next().await.unwrap());
    assert_eq!(msg, Text("421[\"test\",\"foo\"]".to_string()));
    assert_ok!(stx.send(Text("431[\"oof\"]".to_string())).await);

    let ack = rx.recv().await.unwrap();
    assert_eq!(ack[0], "oof");

    let msg = assert_ok!(srx.next().await.unwrap());
    assert_eq!(msg, Text("422[\"test\",\"foo\"]".to_string()));
    assert_ok!(stx.send(Text("432[\"oof\"]".to_string())).await);

    let ack = rx.recv().await.unwrap();
    assert_eq!(ack[0], "oof");

    assert_ok!(stx.close().await);
}

#[tokio::test]
pub async fn broadcast_with_ack() {
    const PORT: u16 = 2101;
    use Message::*;
    let io = create_server(PORT).await;
    let (tx, mut rx) = mpsc::channel::<[String; 1]>(100);

    let io2 = io.clone();
    io.ns("/", move |socket: SocketRef| async move {
        let res = io2.emit_with_ack::<[String; 1]>("test", "foo");
        let sockets = io2.sockets().unwrap();
        let res = assert_ok!(res);
        res.for_each(|(id, res)| {
            let ack = assert_ok!(res);
            assert_ok!(tx.try_send(ack.data));
            assert_some!(sockets.iter().find(|s| s.id == id));
            async move {}
        })
        .await;

        let res = io2
            .timeout(Duration::from_millis(500))
            .emit_with_ack::<[String; 1]>("test", "foo");
        let res = assert_ok!(res);
        res.for_each(|(id, res)| {
            let ack = assert_ok!(res);
            assert_ok!(tx.try_send(ack.data));
            assert_some!(sockets.iter().find(|s| s.id == id));
            async move {}
        })
        .await;

        let res = socket
            .broadcast()
            .timeout(Duration::from_millis(500))
            .emit_with_ack::<[String; 1]>("test", "foo");
        let res = assert_ok!(res);
        res.for_each(|(id, res)| {
            let ack = assert_ok!(res);
            assert_ok!(tx.try_send(ack.data));
            assert_some!(sockets.iter().find(|s| s.id == id));
            async move {}
        })
        .await;
    });

    // Spawn 5 clients and make them echo the ack
    for _ in 0..5 {
        tokio::spawn(async move {
            let (mut stx, mut srx) = create_ws_connection(PORT).await.split();
            assert_ok!(srx.next().await.unwrap());
            assert_ok!(srx.next().await.unwrap());

            while let Some(msg) = srx.next().await {
                let msg = match assert_ok!(msg) {
                    Text(msg) => msg,
                    _ => panic!("Unexpected message"),
                };
                let ack = match assert_ok!(Packet::try_from(msg[1..].to_string())).inner {
                    PacketData::Event(_, _, Some(ack)) => ack,
                    _ => panic!("Unexpected packet"),
                };
                assert_ok!(
                    stx.send(Text(format!("43{}[\"oof\"]", ack.to_string())))
                        .await
                );
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
