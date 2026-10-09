//! The lifecycle of a socket over the public API.

use std::time::Duration;

use socketioxide::{
    SocketIo,
    extract::{AckSender, Data, SocketRef},
};
use socketioxide_client::{IoManager, errors::SendError};
use tokio::{sync::mpsc, time};

/// Wait for `f` to hold, the client connects in the background.
async fn eventually(what: &str, mut f: impl FnMut() -> bool) {
    let waited = time::timeout(Duration::from_secs(5), async {
        while !f() {
            time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;

    assert!(waited.is_ok(), "timed out waiting for {what}");
}

/// A socket is connected to its namespace, gets the id the server gave it there,
/// and its events are dispatched both ways. Once it leaves its namespace it
/// cannot be used anymore, and the next socket gets a connection of its own.
#[tokio::test]
async fn a_socket_connects_emits_and_disconnects() {
    let (svc, server) = SocketIo::new_svc();
    let io = IoManager::new();
    let (tx, mut rx) = mpsc::channel::<String>(4);

    server.ns("/", async move |socket: SocketRef| {
        let tx = tx.clone();
        socket.on(
            "hello",
            async move |Data::<String>(data), ack: AckSender| {
                ack.send(&data).ok();
                tx.send(data).await.ok();
            },
        );
    });

    let socket = io.connect(svc.clone()).await;

    //TODO: thats bad, handle connect frame better.

    // the id comes from the namespace the server accepted it on, it is not the
    // id of the underlying engine.io connection.
    eventually("the socket to be connected", || socket.id().is_some()).await;

    // the event reaches the server, and its acknowledgement comes back.
    let ack = socket
        .emit_with_ack::<_, String>("hello", "world")
        .await
        .expect("the socket should accept the event");

    assert_eq!(ack.await.unwrap(), "world");
    assert_eq!(rx.recv().await.as_deref(), Some("world"));

    socket.disconnect().await.unwrap();

    // it left its namespace: it has no id anymore and nothing can be sent.
    eventually("the socket to be disconnected", || socket.id().is_none()).await;
    let sent = socket.emit("hello", "again").await;
    assert!(matches!(sent, Err(SendError::SocketClosed)), "{sent:?}");

    // its connection was released, so this is a new socket on a new connection.
    let socket2 = io.connect(svc).await;
    assert!(!std::sync::Arc::ptr_eq(&socket, &socket2));
    eventually("the new socket to be connected", || socket2.id().is_some()).await;
    assert_ne!(socket.id(), socket2.id());
}
