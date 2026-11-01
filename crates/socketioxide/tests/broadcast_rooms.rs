//! A socket in several of the targeted rooms must be selected only once.

use std::time::Duration;

use engineioxide::Packet;
use socketioxide::{SocketIo, extract::SocketRef};
use tokio::sync::mpsc;

const TIMEOUT: Duration = Duration::from_millis(100);

#[tokio::test]
async fn emit_to_overlapping_rooms_is_received_once() {
    let (_svc, io) = SocketIo::new_svc();
    io.ns("/", async |s: SocketRef| s.join(["room1", "room2"]));
    let (_stx, mut srx) = io.new_dummy_sock("/", ()).await;
    assert!(matches!(srx.recv().await, Some(Packet::Message(_)))); // connect packet

    io.to(["room1", "room2"]).emit("event", &1).await.unwrap();

    let msg = tokio::time::timeout(TIMEOUT, srx.recv()).await.unwrap();
    assert!(matches!(msg, Some(Packet::Message(m)) if m.contains("event")));
    tokio::time::timeout(TIMEOUT, srx.recv())
        .await
        .expect_err("the event should be received once");
}

#[tokio::test]
async fn sockets_of_overlapping_rooms_are_listed_once() {
    let (_svc, io) = SocketIo::new_svc();
    io.ns("/", async |s: SocketRef| s.join(["room1", "room2"]));
    let (_stx, _srx) = io.new_dummy_sock("/", ()).await;

    assert_eq!(io.to(["room1", "room2"]).sockets().len(), 1);
}

/// The ack stream expects as many acks as selected sockets: it would wait for a second one.
#[tokio::test]
async fn emit_with_ack_to_overlapping_rooms_expects_one_ack() {
    let (_svc, io) = SocketIo::new_svc();
    let (tx, mut rx) = mpsc::channel::<()>(1);
    io.ns("/", async move |s: SocketRef| {
        s.join(["room1", "room2"]);
        tx.try_send(()).unwrap();
    });
    let (_stx, _srx) = io.new_dummy_sock("/", ()).await;
    rx.recv().await.unwrap();

    let stream = io
        .to(["room1", "room2"])
        .timeout(TIMEOUT)
        .emit_with_ack::<_, ()>("event", &1)
        .await
        .unwrap();
    use futures_util::StreamExt;
    let results: Vec<_> = stream.collect().await;
    assert_eq!(results.len(), 1);
}
